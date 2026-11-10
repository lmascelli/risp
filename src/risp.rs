use std::sync::Arc;

use crate::{
    Env, EvalError, LispContext, LispExp, LispPrimitive,
    call_callable, eval, setup_base_env,
};

pub struct Risp<T: LispContext> {
    host: T,
    env: Arc<Env<T>>,
}

impl<T: LispContext> Risp<T> {
    /// Create an interpreter with the standard library installed.
    pub fn new(host: T) -> Self {
        let env = Env::new_root();
        setup_base_env(env.clone());

        Self { host, env }
    }

    /// Evaluate a form in this interpreter's persistent environment.
    pub fn eval_form(
        &mut self,
        form: &LispExp<T>,
    ) -> Result<LispExp<T>, EvalError<T>> {
        eval(form, self.env.clone(), &self.host)
    }

    /// Call a named function with alread evaluated arguments.
    pub fn call(
        &mut self,
        name: &str,
        arguments: &[LispExp<T>],
    ) -> Result<LispExp<T>, EvalError<T>> {
        let callable = self
            .env
            .get_function(name)
            .ok_or_else(|| EvalError::UndefinedFunction(name.to_owned()))?;

        call_callable(
            &callable,
            arguments,
            self.env.clone(),
            &self.host,
        )
    }

    /// Register a primitive, replacing any function with the same name.
    pub fn register_function(
        &mut self,
        name: &str,
        primitive: LispPrimitive<T>,
    ) {
        self.env.set_function(
            name.to_owned(),
            LispExp::primitive(primitive, None),
        );
    }
}