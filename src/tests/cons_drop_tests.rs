#[cfg(test)]
mod tests {
    use crate::LispExp;

    fn on_small_stack(body: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(body)
            .expect("could not create test thread")
            .join()
            .expect("test thread panicked");
    }

    #[test]
    fn dropping_a_deep_car_chain_does_not_overflow() {
        on_small_stack(|| {
            let mut value = LispExp::<()>::nil();

            for _ in 0..100_000 {
                value = LispExp::cons(value, LispExp::nil());
            }

            drop(value);
        });
    }

    #[test]
    fn dropping_a_long_cdr_chain_does_not_overflow() {
        on_small_stack(|| {
            let mut value = LispExp::<()>::nil();

            for _ in 0..100_000 {
                value = LispExp::cons(LispExp::number(1.0), value);
            }

            drop(value);
        });
    }

    #[test]
    fn dropping_an_alternating_chain_does_not_overflow() {
        on_small_stack(|| {
            let mut value = LispExp::<()>::nil();

            for i in 0..100_000 {
                value = if i % 2 == 0 {
                    LispExp::cons(value, LispExp::nil())
                } else {
                    LispExp::cons(LispExp::number(1.0), value)
                };
            }

            drop(value);
        });
    }

    #[test]
    fn dropping_a_parent_preserves_a_shared_child() {
        let child = LispExp::<()>::cons(LispExp::number(42.0), LispExp::nil());

        let parent = LispExp::cons(child.clone(), child.clone());

        drop(parent);

        let LispExp::Cons(cell) = &child else {
            panic!("expected a cons cell");
        };

        assert_eq!(cell.car, LispExp::number(42.0));
        assert!(cell.cdr.is_nil());
        assert_eq!(std::sync::Arc::strong_count(cell), 1);
    }
}
