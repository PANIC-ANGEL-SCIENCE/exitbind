use super::{canonical, text, value};
use serde_json::{json, Map, Value};

// The pre-append implementation is retained as an independent compatibility oracle.
fn former_canonical(input: &Value) -> String {
    match input {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON object key serializes"),
                        former_canonical(&map[key])
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(values) => {
            let values = values
                .iter()
                .map(former_canonical)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{values}]")
        }
        _ => serde_json::to_string(input).expect("JSON value serializes"),
    }
}

fn assert_former_bytes_and_hash(input: &Value, label: &str) {
    let expected = former_canonical(input);
    assert_eq!(canonical(input).as_bytes(), expected.as_bytes(), "{label}");
    assert_eq!(value(input), text(&expected), "{label}");
}

#[test]
fn canonical_literal_bytes_and_hashes_are_stable() {
    let cases = [
        (
            "null",
            Value::Null,
            "null",
            "74234e98afe7498fb5daf1f36ac2d78acc339464f950703b8c019892f982b90b",
        ),
        (
            "nested escapes and Unicode",
            json!({"z": [true, false, null], "a": {"quoted\"key": "line\n\t\\\"", "utf8": "한🌙"}}),
            r#"{"a":{"quoted\"key":"line\n\t\\\"","utf8":"한🌙"},"z":[true,false,null]}"#,
            "c4f73b65107a67cf073331ce82b786500b66dc1eb093583771ab32062a4aa114",
        ),
        (
            "number boundaries and empty object",
            json!({"unsigned": u64::MAX, "signed": i64::MIN, "number": -0.0_f64, "empty": {}}),
            r#"{"empty":{},"number":-0.0,"signed":-9223372036854775808,"unsigned":18446744073709551615}"#,
            "507f3f46107043249927a110faf2074d35cab8888cfa927df4008d7be7cef28d",
        ),
    ];
    for (label, input, expected, expected_hash) in cases {
        assert_eq!(canonical(&input).as_bytes(), expected.as_bytes(), "{label}");
        assert_eq!(value(&input), expected_hash, "{label}");
    }
}

#[test]
fn canonical_matches_former_bytes_and_hashes_for_nested_edge_values() {
    let controls: String = (0_u8..=31).map(char::from).collect();
    let mut keys = Map::new();
    for (key, entry) in [
        ("z", json!(1)),
        ("", json!(2)),
        ("\u{0000}", json!(3)),
        ("\"", json!(4)),
        ("\\", json!(5)),
        ("a\n", json!(6)),
        ("한", json!(7)),
        ("🌙", json!(8)),
    ] {
        keys.insert(key.to_owned(), entry);
    }
    let mut atoms = vec![
        Value::Null,
        json!(false),
        json!(true),
        json!(""),
        json!(controls),
        json!("quotes \" slash / backslash \\ and Unicode 한🌙\u{2028}"),
        json!(i64::MIN),
        json!(i64::MAX),
        json!(u64::MAX),
        json!(0),
        json!([]),
        json!({}),
        Value::Object(keys),
    ];
    for number in [
        0.0_f64,
        -0.0_f64,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::MIN,
        f64::MAX,
        1e-100,
        1e100,
    ] {
        atoms.push(json!(number));
    }
    for (index, atom) in atoms.into_iter().enumerate() {
        assert_former_bytes_and_hash(&atom, &format!("atom {index}"));
        let nested = json!({
            "z": {"inner": atom.clone(), "index": index},
            "a": [atom, {"empty": [], "unicode": "雪"}, null],
        });
        assert_former_bytes_and_hash(&nested, &format!("nested {index}"));
    }
}
