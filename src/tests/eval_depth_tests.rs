#[cfg(test)]
mod test {
    use crate::{Env, EvalError, LispContext, LispExp, Parser, eval, setup_base_env};

    fn eval_script(source: &str) -> Result<LispExp<()>, EvalError<()>> {
        let env = Env::new_root();
        setup_base_env(env.clone());

        let wrapped = format!("(progn {source})");
        let form = Parser::new(&wrapped)
            .next::<()>()
            .expect("invalid test script");

        eval(&form, env, &())
    }

    #[test]
    fn non_tail_recursion_returns_a_depth_error() {
        assert_eq!(
            eval_script(
                "(defun deep (n)
                 (if (= n 0)
                     0
                     (+ 1 (deep (- n 1)))))
             (deep 10000)"
            ),
            Err(EvalError::EvalDepthExceeded),
        );
    }

    #[test]
    fn tail_recursion_does_not_accumulate_depth() {
        assert_eq!(
            eval_script(
                "(defun countdown (n)
                 (if (= n 0)
                     42
                     (countdown (- n 1))))
             (countdown 10000)"
            ),
            Ok(LispExp::number(42.0)),
        );
    }

    #[test]
    fn depth_error_can_be_handled_repeatedly() {
        assert_eq!(
            eval_script(
                "(defun deep (n)
                 (if (= n 0)
                     0
                     (+ 1 (deep (- n 1)))))

             (dotimes (i 100)
                 (condition-case nil
                     (deep 10000)
                   (eval-depth-exceeded nil)))

             42"
            ),
            Ok(LispExp::number(42.0)),
        );
    }

    #[test]
    fn depth_error_does_not_break_the_next_evaluation() {
        assert_eq!(
            eval_script(
                "(defun deep (n)
                 (if (= n 0)
                     0
                     (+ 1 (deep (- n 1)))))
             (deep 10000)"
            ),
            Err(EvalError::EvalDepthExceeded),
        );

        assert_eq!(eval_script("(+ 20 22)"), Ok(LispExp::number(42.0)),);
    }

    #[test]
    fn depth_limit_protects_a_two_megabyte_thread() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                assert_eq!(
                    eval_script(
                        "(defun deep (n)
                         (if (= n 0)
                             0
                             (+ 1 (deep (- n 1)))))
                     (deep 10000)"
                    ),
                    Err(EvalError::EvalDepthExceeded),
                );
            })
            .expect("could not create test thread")
            .join()
            .expect("test thread panicked");
    }
}
