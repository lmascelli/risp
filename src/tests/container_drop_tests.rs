#[cfg(test)]
mod tests {
    use crate::LispExp;
    use std::collections::HashMap;

    fn check_deep_drop(kind: usize) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let mut value = LispExp::<()>::number(1.0);

                for i in 0..100_000 {
                    let container = if kind == 3 { i % 4 } else { kind };

                    value = match container {
                        0 => LispExp::vec(vec![value]),

                        1 => LispExp::form(vec![value]),

                        2 => LispExp::map(HashMap::from([("child".into(), value)])),

                        _ => LispExp::cons(value, LispExp::nil()),
                    };
                }

                drop(value);
            })
            .expect("could not create test thread")
            .join()
            .expect("test thread panicked");
    }

    #[test]
    fn dropping_deep_vectors_does_not_overflow() {
        check_deep_drop(0);
    }

    #[test]
    fn dropping_deep_forms_does_not_overflow() {
        check_deep_drop(1);
    }

    #[test]
    fn dropping_deep_maps_does_not_overflow() {
        check_deep_drop(2);
    }

    #[test]
    fn dropping_mixed_containers_does_not_overflow() {
        check_deep_drop(3);
    }

    #[test]
    fn dropping_a_container_preserves_shared_children() {
        let child = LispExp::<()>::vec(vec![LispExp::number(42.0)]);

        let parent = LispExp::map(HashMap::from([
            ("first".into(), child.clone()),
            ("second".into(), child.clone()),
        ]));

        drop(parent);

        let LispExp::Vector(items) = &child else {
            panic!("expected a vector");
        };

        assert_eq!(items.len(), 1);
        assert_eq!(items[0], LispExp::number(42.0));
        assert_eq!(std::sync::Arc::strong_count(items), 1);
    }
}
