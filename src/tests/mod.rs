#[cfg(test)]
mod tests {
    impl crate::LispContext for () {
        fn consume_fuel(&self, _amount: u32) -> Result<(), crate::EvalError<()>> {
            Ok(())
        }

        fn log_diagnostic(&self, _msg: &str) {}
    }
}

mod backtrace_tests;
mod base_env_tests;
mod cons_drop_tests;
mod container_drop_tests;
mod documentation_tests;
mod equality_tests;
mod eval_depth_tests;
mod eval_tests;
mod fiber_tests;
mod fuel;
mod handshake;
mod lexer_tests;
mod lexical_context;
mod lisp_core_compliance_tests;
mod nonlocal_exit_tests;
mod parser_tests;
mod perf;
mod printing_tests;
mod source_map_tests;
mod symbol_tests;
mod thread_tests;
mod yield_tests;
