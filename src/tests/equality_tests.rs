#[cfg(test)]
mod tests {
    use crate::LispExp;
    use std::collections::HashMap;

    fn nested_list(depth: usize, leaf: f64) -> LispExp<()> {
        let mut value = LispExp::number(leaf);

        for _ in 0..depth {
            value = LispExp::cons(value, LispExp::nil());
        }

        value
    }

    #[test]
    fn equality_handles_deeply_nested_lists() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let left = nested_list(100_000, 1.0);
                let right = nested_list(100_000, 1.0);
                let different = nested_list(100_000, 2.0);

                // Avoid formatting deep values if an assertion fails.
                assert!(left == right);
                assert!(left != different);
            })
            .expect("could not create test thread")
            .join()
            .expect("test thread panicked");
    }

    #[test]
    fn a_shared_child_does_not_hide_a_different_tail() {
        let shared = LispExp::<()>::proper_list(vec![
            LispExp::number(42.0),
        ]);

        let left = LispExp::cons(
            shared.clone(),
            LispExp::number(1.0),
        );

        let right = LispExp::cons(
            shared,
            LispExp::number(2.0),
        );

        assert!(left != right);
    }

    #[test]
    fn equality_checks_dotted_tails() {
        let left = LispExp::<()>::cons(
            LispExp::number(1.0),
            LispExp::number(2.0),
        );

        let equal = LispExp::cons(
            LispExp::number(1.0),
            LispExp::number(2.0),
        );

        let different = LispExp::cons(
            LispExp::number(1.0),
            LispExp::number(3.0),
        );

        assert!(left == equal);
        assert!(left != different);
    }

    #[test]
    fn equality_traverses_forms_vectors_and_maps() {
        fn make_value(leaf: f64) -> LispExp<()> {
            let mut map = HashMap::new();

            map.insert(
                "key".into(),
                LispExp::proper_list(vec![LispExp::number(leaf)]),
            );

            LispExp::form(vec![
                LispExp::vec(vec![LispExp::map(map)]),
            ])
        }

        assert!(make_value(1.0) == make_value(1.0));
        assert!(make_value(1.0) != make_value(2.0));
    }

    #[test]
    fn equality_checks_container_lengths_and_map_keys() {
        let short = LispExp::<()>::vec(vec![]);
        let long = LispExp::vec(vec![LispExp::number(1.0)]);

        assert!(short != long);

        let left = LispExp::<()>::map(HashMap::from([
            ("left".into(), LispExp::number(1.0)),
        ]));

        let right = LispExp::map(HashMap::from([
            ("right".into(), LispExp::number(1.0)),
        ]));

        assert!(left != right);
    }

    #[test]
    fn equality_preserves_nan_behavior() {
        let vector = LispExp::<()>::vec(vec![
            LispExp::number(f64::NAN),
        ]);

        assert!(vector != vector.clone());

        // Existing cons equality recognizes the same allocation.
        let cons = LispExp::<()>::cons(
            LispExp::number(f64::NAN),
            LispExp::nil(),
        );

        assert!(cons == cons.clone());
    }
}