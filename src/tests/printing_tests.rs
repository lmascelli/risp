#[cfg(test)]
mod test {
    use crate::{LispExp, lisp_display};
    use std::collections::HashMap;

    #[test]
    fn printing_handles_deeply_nested_lists() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let depth = 100_000;
                let mut value = LispExp::<()>::number(7.0);

                for _ in 0..depth {
                    value = LispExp::cons(value, LispExp::nil());
                }

                let text = format!("{:?}", value);
                let bytes = text.as_bytes();

                assert_eq!(bytes.len(), depth * 2 + 1);
                assert!(bytes[..depth].iter().all(|b| *b == b'('));
                assert_eq!(bytes[depth], b'7');
                assert!(bytes[depth + 1..].iter().all(|b| *b == b')'));

                assert_eq!(lisp_display(&value), text);
            })
            .expect("could not create test thread")
            .join()
            .expect("test thread panicked");
    }

    #[test]
    fn printing_preserves_collection_notation() {
        let value = LispExp::<()>::form(vec![
            LispExp::string("a".into()),
            LispExp::vec(vec![LispExp::number(1.0)]),
            LispExp::cons(LispExp::number(2.0), LispExp::number(3.0)),
        ]);

        assert_eq!(format!("{:?}", value), r#"("a" [1] (2 . 3))"#,);
    }

    #[test]
    fn printing_preserves_map_notation() {
        let value = LispExp::<()>::map(HashMap::from([(
            "k".into(),
            LispExp::vec(vec![LispExp::number(1.0)]),
        )]));

        assert_eq!(format!("{:?}", value), "{k [1]}");
    }

    #[test]
    fn printing_preserves_empty_values_and_proper_lists() {
        assert_eq!(format!("{:?}", LispExp::<()>::nil()), "nil");
        assert_eq!(format!("{:?}", LispExp::<()>::form(vec![])), "()");
        assert_eq!(format!("{:?}", LispExp::<()>::vec(vec![])), "[]");
        assert_eq!(format!("{:?}", LispExp::<()>::map(HashMap::new())), "{}",);

        let list = LispExp::<()>::proper_list(vec![LispExp::number(1.0), LispExp::number(2.0)]);

        assert_eq!(format!("{:?}", list), "(1 2)");
    }
}
