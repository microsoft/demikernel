// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum OptionValue {
    Bool(bool),
    Float(f64),
    Int(i64),
    String(String),
}

#[derive(Clone, Debug)]
pub struct Options {
    inner: HashMap<String, OptionValue>,
}

impl Options {
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.inner.get(key).map(|v| match v {
            OptionValue::Bool(b) => *b,
            _ => panic!("Value for {} should be a bool", key),
        })
    }

    pub fn insert_bool(&mut self, key: String, value: bool) {
        self.inner.insert(key, OptionValue::Bool(value));
    }

    pub fn get_float(&self, key: &str) -> Option<f64> {
        self.inner.get(key).map(|v| match v {
            OptionValue::Float(f) => *f,
            _ => panic!("Value for {} should be a float", key),
        })
    }

    pub fn insert_float(&mut self, key: String, value: f64) {
        self.inner.insert(key, OptionValue::Float(value));
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.inner.get(key).map(|v| match v {
            OptionValue::Int(i) => *i,
            _ => panic!("Value for {} should be an int", key),
        })
    }

    pub fn insert_int(&mut self, key: String, value: i64) {
        self.inner.insert(key, OptionValue::Int(value));
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.inner.get(key).map(|v| match v {
            OptionValue::String(s) => s.clone(),
            _ => panic!("Value for {} should be a string", key),
        })
    }

    pub fn insert_string(&mut self, key: String, value: String) {
        self.inner.insert(key, OptionValue::String(value));
    }
}

impl Default for Options {
    fn default() -> Self {
        Self { inner: HashMap::new() }
    }
}
