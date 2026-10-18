use super::{Frame, LispContext, LispExp};

/// Evaluation failures and non-local control transfers.
///
/// `Throw` and `Yielded` carry control flow for `catch` and fiber resumption;
/// they are not ordinary signaled conditions. Other variants describe invalid
/// forms, invalid arguments, host fuel exhaustion, or runtime failures.
#[derive(Debug, PartialEq)]
pub enum EvalError<T: LispContext> {
    /// A variable lookup found no binding for the contained name.
    UnboundVariable(String),
    /// A function lookup found no binding for the contained name.
    UndefinedFunction(String),
    /// The expression cannot be used as a function call, for example an improper code list.
    UnvalidFunctionCall,
    /// A function binding is not callable, or a runtime verification or fiber lock failed.
    UncorrectFunctionDefinition,
    /// The supplied argument count falls outside the accepted count or range.
    WrongNumberOfArguments {
        /// Required count, or the violated lower or upper bound for a range.
        expected: usize,
        /// Number of supplied arguments.
        got: usize,
    },
    /// An argument has an unsupported Lisp value type.
    WrongArgumentType {
        /// Human-readable type requirement.
        expected: String,
        /// Offending value, preserved for Lisp condition handlers.
        got: LispExp<T>,
    },
    /// `quote` was given a number of arguments other than one.
    QuoteNotOneArgument,
    /// `if` has no condition expression.
    IfNoConditionProvided,
    /// `if` has a condition but no expression for its true branch.
    IfNoTrueBrach,
    /// A variable name in `setq` is not a symbol.
    SetqSymbolRequired,
    /// `setq` lacks complete name/value pairs; contains the supplied argument count.
    SetqWrongNumberOfArgs(usize),
    /// A definition requires a symbol name.
    DefunNameMustBeASymbol,
    /// A function, lambda, or macro definition lacks its required forms.
    DefunNotCorrectExpression,
    /// The parameter declaration is not a syntax list.
    DefunParamsAreNotAList,
    /// A parameter declaration contains a non-symbol value.
    DefunParamIsNotASymbol,
    /// `&rest` is not followed by exactly one parameter name.
    DefunRestMustHaveExactlyOneParam,
    /// An `&optional` or `&rest` marker is repeated or out of order.
    DefunMisplacedParamMarker,
    /// A `let` or `let*` binding is malformed; contains its zero-based index.
    LetUnvalidBindingAt(usize),
    /// The binding declaration for `let` or `let*` is neither a syntax list nor nil.
    LetUnvalidBindingList,
    /// `let` or `let*` was called without a binding declaration.
    LetNoBindingsProvided,
    /// A `cond` clause is not a nonempty syntax list.
    CondInvalidClause,
    /// The `dolist` declaration is not `(SYMBOL LIST-FORM [RESULT-FORM])`.
    DolistInvalidBinding,
    /// The `dotimes` declaration is not `(SYMBOL COUNT-FORM [RESULT-FORM])`.
    DotimesInvalidBinding,
    /// A variable definition lacks its name.
    DefvarNameMustBeASymbol,
    /// A variable definition supplies a docstring that is not a string literal.
    DefvarDocMustBeAString,
    /// `backquote` was given a number of arguments other than one.
    BackquoteNotOneArgument,
    /// The host refused an execution charge.
    OutOfFuel,
    /// The handler variable of `condition-case` is neither a symbol nor nil.
    ConditionCaseInvalidVariable,
    /// A `condition-case` handler is not a nonempty syntax list.
    ConditionCaseInvalidHandler,
    /// A tagged value traveling to a matching Lisp `catch`.
    Throw {
        /// Tag matched against enclosing `catch` forms.
        tag: LispExp<T>,
        /// Value returned by the matching `catch`.
        value: LispExp<T>,
    },
    /// A yielded value and continuation, with the innermost frame first.
    Yielded {
        /// Value returned to the current `resume` caller.
        value: LispExp<T>,
        /// Remaining continuation, innermost block first.
        frames: Vec<Frame<T>>,
    },
    /// A yield occurred outside a resumable statement position.
    YieldNotAllowed,
    /// An explicit Lisp condition and its associated data.
    Signal {
        /// Condition matched against handler declarations.
        symbol: LispExp<T>,
        /// Payload made available to the matching handler.
        data: LispExp<T>,
    },
    /// A runtime or host failure described by the contained message.
    RuntimeMessage(String),
}
