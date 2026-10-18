//! Lexical environments with separate variable, function, and macro namespaces.

use super::{LispContext, LispExp};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Properties indexed by symbol name, then property key.
type PropertyTable<T> = RwLock<HashMap<String, HashMap<String, LispExp<T>>>>;

/// Property key for variable docstrings, written by `defvar` and read by `variable-doc`.
pub const VARIABLE_DOCUMENTATION: &str = "variable-documentation";

/// The namespace used by shared binding and lookup operations.
#[derive(Clone, Copy)]
enum Namespace {
    Variables,
    Functions,
    Macros,
}

/// A lexical scope with independent variable, function, and macro bindings.
///
/// Lookups search the current scope before its parents. Symbol properties are
/// global to the root. Prefer the accessors to direct table mutation.
///
/// # Panics
///
/// Binding and property accessors panic if the corresponding lock is poisoned.
#[derive(Debug)]
pub struct Env<T: LispContext> {
    /// Variable bindings local to this scope.
    pub variables: RwLock<HashMap<String, LispExp<T>>>,
    /// Function bindings local to this scope.
    pub functions: RwLock<HashMap<String, LispExp<T>>>,
    /// Macro bindings local to this scope.
    pub macros: RwLock<HashMap<String, LispExp<T>>>,
    /// Global symbol properties, owned by the root only. Children store `None`;
    /// property accessors walk to the root, so lexical shadowing does not hide properties.
    pub properties: Option<Box<PropertyTable<T>>>,
    /// Enclosing lexical scope; `None` for the root.
    pub parent: Option<Arc<Env<T>>>,
}

impl<T: LispContext> Env<T> {
    /// Create an empty root environment without standard functions.
    pub fn new_root() -> Arc<Self> {
        Arc::new(Self {
            variables: RwLock::new(HashMap::new()),
            functions: RwLock::new(HashMap::new()),
            macros: RwLock::new(HashMap::new()),
            properties: Some(Box::new(RwLock::new(HashMap::new()))),
            parent: None,
        })
    }

    /// Create an empty child scope that inherits bindings from `parent`.
    pub fn new_child(parent: &Arc<Env<T>>) -> Arc<Self> {
        Arc::new(Self {
            variables: RwLock::new(HashMap::new()),
            functions: RwLock::new(HashMap::new()),
            macros: RwLock::new(HashMap::new()),
            properties: None,
            parent: Some(parent.clone()),
        })
    }

    /// Find the root-owned property table without holding locks along the parent chain.
    fn property_owner(&self) -> &Env<T> {
        let mut env = self;
        loop {
            if env.properties.is_some() {
                return env;
            }
            match env.parent.as_deref() {
                Some(parent) => env = parent,
                None => return env,
            }
        }
    }

    /// Set a symbol property in the root, replacing any previous value.
    pub fn put_property(&self, symbol: &str, key: &str, value: LispExp<T>) {
        let owner = self.property_owner();
        if let Some(properties) = &owner.properties {
            properties
                .write()
                .expect("Failed to acquire write lock on properties")
                .entry(symbol.to_string())
                .or_default()
                .insert(key.to_string(), value);
        }
    }

    /// Clone a symbol property from the root, or return `None` if absent.
    pub fn get_property(&self, symbol: &str, key: &str) -> Option<LispExp<T>> {
        let owner = self.property_owner();
        let properties = owner.properties.as_ref()?;
        let table = properties
            .read()
            .expect("Failed to acquire read lock on properties");
        table.get(symbol)?.get(key).cloned()
    }

    fn table(&self, namespace: Namespace) -> &RwLock<HashMap<String, LispExp<T>>> {
        match namespace {
            Namespace::Variables => &self.variables,
            Namespace::Functions => &self.functions,
            Namespace::Macros => &self.macros,
        }
    }

    /// Find the nearest binding, releasing each lock before visiting its parent.
    fn lookup(&self, namespace: Namespace, name: &str) -> Option<LispExp<T>> {
        let mut env = self;
        loop {
            let found = env
                .table(namespace)
                .read()
                .expect("Failed to acquire read lock on env")
                .get(name)
                .cloned();
            if found.is_some() {
                return found;
            }
            env = env.parent.as_deref()?;
        }
    }

    fn bind(&self, namespace: Namespace, name: String, val: LispExp<T>) {
        self.table(namespace)
            .write()
            .expect("Failed to acquire write lock on env")
            .insert(name, val);
    }

    /// Clone the nearest variable binding, or return `None` if unbound.
    pub fn get_variable(&self, name: &str) -> Option<LispExp<T>> {
        self.lookup(Namespace::Variables, name)
    }

    /// Return a finite numeric binding greater than or equal to `floor`.
    ///
    /// Returns `None` for an unbound name, another value type, or an invalid number.
    pub fn number_at_least(&self, name: &str, floor: f64) -> Option<f64> {
        match self.get_variable(name) {
            Some(LispExp::Number(n)) if n.is_finite() && n >= floor => Some(n),
            _ => None,
        }
    }

    /// Return the binding's Lisp truth value, or `default` if unbound.
    ///
    /// A binding to `nil` returns `false`, even when `default` is `true`.
    pub fn flag(&self, name: &str, default: bool) -> bool {
        match self.get_variable(name) {
            Some(value) => value.is_truthy(),
            None => default,
        }
    }

    /// Replace the nearest existing variable binding.
    ///
    /// Returns `false` if unbound; no binding is created in that case.
    pub fn update_variable(&self, name: &str, val: LispExp<T>) -> bool {
        if self
            .variables
            .read()
            .expect("Failed to acquire read lock on env")
            .contains_key(name)
        {
            self.variables
                .write()
                .expect("Failed to acquire write lock on env")
                .insert(name.to_string(), val);
            return true;
        }
        if let Some(parent) = &self.parent {
            return parent.update_variable(name, val);
        }
        false
    }

    /// Clone the nearest function binding, or return `None` if unbound.
    pub fn get_function(&self, name: &str) -> Option<LispExp<T>> {
        self.lookup(Namespace::Functions, name)
    }

    /// Clone the nearest macro binding, or return `None` if unbound.
    pub fn get_macro(&self, name: &str) -> Option<LispExp<T>> {
        self.lookup(Namespace::Macros, name)
    }

    /// Bind a macro in this scope, replacing any local binding.
    pub fn set_macro(&self, name: String, val: LispExp<T>) {
        self.bind(Namespace::Macros, name, val);
    }

    /// Bind a variable in this scope, replacing any local binding.
    ///
    /// Use [`Self::update_variable`] to modify an existing binding in a parent.
    pub fn set_variable(&self, name: String, val: LispExp<T>) {
        self.bind(Namespace::Variables, name, val);
    }

    fn root(&self) -> &Env<T> {
        let mut env = self;
        while let Some(parent) = env.parent.as_deref() {
            env = parent;
        }
        env
    }

    /// Bind a variable in the root environment.
    ///
    /// Unlike [`Self::set_variable`], this writes to the root even when called
    /// from a child. A nearer binding can still shadow the new root value.
    pub fn set_root_variable(&self, name: String, val: LispExp<T>) {
        self.root().set_variable(name, val);
    }

    /// Bind a function in this scope, replacing any local binding.
    pub fn set_function(&self, name: String, val: LispExp<T>) {
        self.bind(Namespace::Functions, name, val);
    }

    /// Return visible variable names, sorted and deduplicated across parent scopes.
    pub fn variable_names(&self) -> Vec<String> {
        self.names_in(Namespace::Variables)
    }

    /// Return visible function names, sorted and deduplicated across parent scopes.
    pub fn function_names(&self) -> Vec<String> {
        self.names_in(Namespace::Functions)
    }

    /// Return visible macro names, sorted and deduplicated across parent scopes.
    ///
    /// Macros occupy a separate namespace from functions.
    pub fn macro_names(&self) -> Vec<String> {
        self.names_in(Namespace::Macros)
    }

    fn names_in(&self, namespace: Namespace) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_names(namespace, &mut names);
        names.sort();
        names.dedup();
        names
    }

    /// Collect names, releasing each lock before recursing into the parent.
    fn collect_names(&self, namespace: Namespace, into: &mut Vec<String>) {
        into.extend(
            self.table(namespace)
                .read()
                .expect("Failed to acquire read lock on env")
                .keys()
                .cloned(),
        );
        if let Some(parent) = &self.parent {
            parent.collect_names(namespace, into);
        }
    }
}

/// Compare environments by identity; structural comparison could traverse closure cycles.
impl<T: LispContext> PartialEq for Env<T> {
    fn eq(&self, other: &Env<T>) -> bool {
        std::ptr::eq(self, other)
    }
}
