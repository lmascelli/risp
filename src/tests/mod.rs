#[cfg(test)]
mod tests {
    impl crate::LispContext for () {
        fn consume_fuel(&self, _amount: u32) -> Result<(), crate::EvalError<()>> {
            Ok(())
        }

        fn log_diagnostic(&self, _msg: &str) {}
    }
}

// Parser tests
mod lexer_tests;
mod parser_tests;
mod source_map_tests;
// Eval tests
mod backtrace_tests;
mod base_env_tests;
mod documentation_tests;
mod eval_tests;
mod lexical_context;
mod lisp_core_compliance_tests;
mod nonlocal_exit_tests;
mod symbol_tests;
// Thread, concurrency and fuel
mod fiber_tests;
mod fuel;
#[cfg(test)]
mod perf;
mod thread_tests;
mod yield_tests;
// Handshake
mod handshake;
