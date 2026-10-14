use super::{Frame, LispContext, LispExp};

#[derive(Debug, PartialEq)]
pub enum EvalError<T: LispContext> {
    UnboundVariable(String),
    UndefinedFunction(String),
    UnvalidFunctionCall,
    UncorrectFunctionDefinition,
    WrongNumberOfArguments {
        expected: usize,
        got: usize,
    },
    WrongArgumentType {
        expected: String,
        got: LispExp<T>,
    },
    QuoteNotOneArgument,
    IfNoConditionProvided,
    IfNoTrueBrach,
    SetqSymbolRequired,
    SetqWrongNumberOfArgs(usize),
    DefunNameMustBeASymbol,
    DefunNotCorrectExpression,
    DefunParamsAreNotAList,
    DefunParamIsNotASymbol,
    DefunRestMustHaveExactlyOneParam,
    DefunMisplacedParamMarker,
    LetUnvalidBindingAt(usize),
    LetUnvalidBindingList,
    LetNoBindingsProvided,
    CondInvalidClause,
    DolistInvalidBinding,
    DotimesInvalidBinding,
    DefvarNameMustBeASymbol,
    DefvarDocMustBeAString,
    BackquoteNotOneArgument,
    OutOfFuel,
    ConditionCaseInvalidVariable,
    ConditionCaseInvalidHandler,
    Throw {
        tag: LispExp<T>,
        value: LispExp<T>,
    },
    Yielded {
        value: LispExp<T>,
        frames: Vec<Frame<T>>,
    },
    YieldNotAllowed,
    Signal {
        symbol: LispExp<T>,
        data: LispExp<T>,
    },
    RuntimeMessage(String),
}
