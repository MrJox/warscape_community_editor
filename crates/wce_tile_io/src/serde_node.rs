//! Any serde type (rpfm_lib's file structs: BMD, the civilian lists, the tile database ...) as a model node tree and back, through
//! `serde_json::Value`.
//!
//! An object is a node of the field's name: its scalar fields are properties (`Bool`, `Int`, `Float`, `Str`), arrays of numbers are `Ints`
//! or `Floats` properties (an empty array is an empty `Floats`), objects and other arrays are child nodes; a child node made from an array
//! has the property `array` (true) and one `item` child per element (a scalar element: `item` with the property `value`); `null` is a child
//! node with the property `null` (true). Integers above `i64::MAX` are kept as `Str` with a `u64:` prefix. serde_json writes a float NaN or
//! infinity as `null`: such a struct does not come back (the caller checks its round trip).

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Number, Value as Json};
use wce_tile_model::{Node, Value};

fn number(n: &Number) -> Value {
    if let Some(i) = n.as_i64() {
        Value::Int(i)
    } else if let Some(u) = n.as_u64() {
        Value::Str(format!("u64:{u}"))
    } else {
        Value::Float(n.as_f64().unwrap_or(f64::NAN))
    }
}

fn to_child(name: &str, v: &Json) -> Node {
    match v {
        Json::Object(map) => object(name, map),
        Json::Array(items) => {
            let mut n = Node::new(name);
            n.set("array", Value::Bool(true));
            n.children = items.iter().map(item).collect();
            n
        }

        Json::Null => {
            let mut n = Node::new(name);
            n.set("null", Value::Bool(true));
            n
        }

        scalar => {
            let mut n = Node::new(name);
            n.set("value", scalar_value(scalar).expect("a scalar"));
            n
        }
    }
}

fn item(v: &Json) -> Node {
    to_child("item", v)
}

fn scalar_value(v: &Json) -> Option<Value> {
    Some(match v {
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => number(n),
        Json::String(s) => Value::Str(s.clone()),
        _ => return None,
    })
}

/// A number array as one property, when every element is a number of one kind.
fn number_array(items: &[Json]) -> Option<Value> {
    if items.is_empty() {
        return Some(Value::Floats(vec![]));
    }

    if items.iter().all(|i| i.as_i64().is_some() && !i.as_number().is_some_and(Number::is_f64)) {
        return Some(Value::Ints(items.iter().map(|i| i.as_i64().expect("an i64")).collect()));
    }

    if items.iter().all(|i| i.as_number().is_some_and(Number::is_f64)) {
        return Some(Value::Floats(items.iter().map(|i| i.as_f64().expect("an f64")).collect()));
    }

    None
}

fn object(name: &str, map: &Map<String, Json>) -> Node {
    let mut n = Node::new(name);
    for (k, v) in map {
        if let Some(s) = scalar_value(v) {
            n.set(k.as_str(), s);
        } else if let Json::Array(items) = v
            && let Some(arr) = number_array(items)
        {
            n.set(k.as_str(), arr);
        } else {
            n.children.push(to_child(k, v));
        }
    }

    n
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) => Json::from(*i),
        Value::Float(f) => Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Value::Str(s) => match s.strip_prefix("u64:").and_then(|u| u.parse::<u64>().ok()) {
            Some(u) => Json::from(u),
            None => Json::String(s.clone()),
        },

        Value::Floats(f) => Json::Array(f.iter().map(|x| Number::from_f64(*x).map_or(Json::Null, Json::Number)).collect()),
        Value::Ints(i) => Json::Array(i.iter().map(|x| Json::from(*x)).collect()),
        other => Json::String(format!("{other:?}")),
    }
}

fn node_json(n: &Node) -> Json {
    if n.get("null").and_then(Value::as_bool) == Some(true) {
        return Json::Null;
    }

    if n.get("array").and_then(Value::as_bool) == Some(true) {
        return Json::Array(n.children.iter().map(node_json).collect());
    }

    if n.props().len() == 1 && n.children.is_empty() && n.props()[0].name == "value" {
        return value_json(&n.props()[0].value);
    }

    let mut map = Map::new();
    for p in n.props() {
        map.insert(p.name.as_str().to_owned(), value_json(&p.value));
    }

    for c in &n.children {
        map.insert(c.class.as_str().to_owned(), node_json(c));
    }

    Json::Object(map)
}

/// A value as a node tree whose root has the class `name`.
///
/// # Errors
///
/// The value does not serialise, or is not a struct.
pub fn to_node<T: Serialize>(name: &str, value: &T) -> Result<Node, String> {
    match serde_json::to_value(value).map_err(|e| e.to_string())? {
        Json::Object(map) => Ok(object(name, &map)),
        other => Ok(to_child(name, &other)),
    }
}

/// The value a node tree made by [`to_node`] holds.
///
/// # Errors
///
/// The tree does not fit the type.
pub fn from_node<T: DeserializeOwned>(node: &Node) -> Result<T, String> {
    serde_json::from_value(node_json(node)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use serde_derive::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Inner {
        name: String,
        points: Vec<f32>,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Outer {
        version: u16,
        big: u64,
        flag: bool,
        scale: f32,
        ids: Vec<u32>,
        empty: Vec<f32>,
        inners: Vec<Inner>,
        names: Vec<String>,
        maybe: Option<Inner>,
        inner: Inner,
    }

    #[test]
    fn round_trip() {
        let v = Outer {
            version: 6, big: u64::MAX, flag: true, scale: 0.1, ids: vec![1, 2], empty: vec![], names: vec!["a".into()], maybe: None,
            inners: vec![Inner { name: "x".into(), points: vec![1.5, -0.25] }], inner: Inner { name: "y".into(), points: vec![] },
        };

        let n = to_node("Outer", &v).unwrap();
        assert_eq!(n.get("scale"), Some(&Value::Float(f64::from(0.1f32))));
        assert_eq!(n.get("ids"), Some(&Value::Ints(vec![1, 2])));
        assert_eq!(from_node::<Outer>(&n).unwrap(), v);
    }
}
