// Copyright 2019 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::BTreeMap,
    io::{Error, ErrorKind, Result, Write},
};

use serde::Serialize;
use serde_json::ser::{CharEscape, CompactFormatter, Formatter, Serializer};
use unicode_normalization::UnicodeNormalization;

/// A [`serde_json::ser::Formatter`] which outputs [Canonical JSON].
///
/// The Radicle Link specification disagrees with [Canonical JSON] spec, in that
/// ASCII plane control characters (`U+0000` - `U+007F`) string values are
/// escaped according to the normal JSON escaping rules as per [RFC 8259],
/// Section 7. The reason is that conformant JSON parsers (such as `serde_json`)
/// will typically refuse to accept strings containing these characters as
/// unescaped bytes. As we standardise on an exact set, and mandate that hex
/// escape sequences shall be lower-case, canonicity is preserved (unicode
/// normalisation still applies).
///
/// This implementation is based on the [`olpc-cjson`] crate, and inlined here
/// for distribution convenience. We expressly license the original code under
/// the term of the MIT licence.
///
/// [Canonical JSON]: http://wiki.laptop.org/go/Canonical_JSON
/// [RFC 8259]: https://www.rfc-editor.org/rfc/rfc8259.txt
#[derive(Debug, Default)]
pub struct CanonicalFormatter {
    object_stack: Vec<Object>,
}

/// Internal struct to keep track of an object in progress of being built.
///
/// As keys and values are received by `CanonicalFormatter`, they are written to
/// `next_key` and `next_value` by using the `CanonicalFormatter::writer`
/// convenience method.
///
/// How this struct behaves when `Formatter` methods are called:
///
/// ```plain
/// [other methods]  // values written to the writer received by method
/// begin_object     // create this object
/// /-> begin_object_key    // object.key_done = false;
/// |   [other methods]     // values written to object.next_key, writer received by method ignored
/// |   end_object_key      // object.key_done = true;
/// |   begin_object_value  // [nothing]
/// |   [other methods]     // values written to object.next_value
/// |   end_object_value    // object.next_key and object.next_value are inserted into object.obj
/// \---- // jump back if more values are present
/// end_object       // write the object (sorted by its keys) to the writer received by the method
/// ```
#[derive(Debug, Default)]
struct Object {
    obj: BTreeMap<Vec<u8>, Vec<u8>>,
    next_key: Vec<u8>,
    next_value: Vec<u8>,
    key_done: bool,
}

impl CanonicalFormatter {
    /// Create a new `CanonicalFormatter` object.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience method to return the appropriate writer given the current
    /// context.
    ///
    /// If we are currently writing an object (that is, if
    /// `!self.object_stack.is_empty()`), we need to write the value to
    /// either the next key or next value depending on that state
    /// machine. See the docstrings for `Object` for more detail.
    ///
    /// If we are not currently writing an object, pass through `writer`.
    fn writer<'a, W: Write + ?Sized>(&'a mut self, writer: &'a mut W) -> Box<dyn Write + 'a> {
        if let Some(object) = self.object_stack.last_mut() {
            if object.key_done {
                Box::new(&mut object.next_value)
            } else {
                Box::new(&mut object.next_key)
            }
        } else {
            Box::new(writer)
        }
    }

    /// Returns a mutable reference to the top of the object stack.
    fn obj_mut(&mut self) -> Result<&mut Object> {
        self.object_stack.last_mut().ok_or_else(|| {
            Error::new(
                ErrorKind::Other,
                "serde_json called an object method without calling begin_object first",
            )
        })
    }
}

/// Wraps `serde_json::CompactFormatter` to use the appropriate writer (see
/// `CanonicalFormatter::writer`).
macro_rules! wrapper {
    ($f:ident) => {
        fn $f<W: Write + ?Sized>(&mut self, writer: &mut W) -> Result<()> {
            CompactFormatter.$f(&mut self.writer(writer))
        }
    };

    ($f:ident, $t:ty) => {
        fn $f<W: Write + ?Sized>(&mut self, writer: &mut W, arg: $t) -> Result<()> {
            CompactFormatter.$f(&mut self.writer(writer), arg)
        }
    };
}

/// This is used in three places. Write it once.
macro_rules! float_err {
    () => {
        Err(Error::new(
            ErrorKind::InvalidInput,
            "floating point numbers are not allowed in canonical JSON",
        ))
    };
}

impl Formatter for CanonicalFormatter {
    wrapper!(write_null);
    wrapper!(write_bool, bool);
    wrapper!(write_i8, i8);
    wrapper!(write_i16, i16);
    wrapper!(write_i32, i32);
    wrapper!(write_i64, i64);
    wrapper!(write_u8, u8);
    wrapper!(write_u16, u16);
    wrapper!(write_u32, u32);
    wrapper!(write_u64, u64);

    fn write_f32<W: Write + ?Sized>(&mut self, _writer: &mut W, _value: f32) -> Result<()> {
        float_err!()
    }

    fn write_f64<W: Write + ?Sized>(&mut self, _writer: &mut W, _value: f64) -> Result<()> {
        float_err!()
    }

    // By default this is only used for u128/i128. If serde_json's
    // `arbitrary_precision` feature is enabled, all numbers are internally
    // stored as strings, and this method is always used (even for floating
    // point values).
    fn write_number_str<W: Write + ?Sized>(&mut self, writer: &mut W, value: &str) -> Result<()> {
        if value.chars().any(|c| c == '.' || c == 'e' || c == 'E') {
            float_err!()
        } else {
            CompactFormatter.write_number_str(&mut self.writer(writer), value)
        }
    }

    wrapper!(begin_string);
    wrapper!(end_string);

    // Strings are normalized as Normalization Form C (NFC). `str::nfc` is provided
    // by the `UnicodeNormalization` trait and returns an iterator of `char`s.
    fn write_string_fragment<W: Write + ?Sized>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> Result<()> {
        fragment.nfc().try_for_each(|ch| {
            self.writer(writer)
                .write_all(ch.encode_utf8(&mut [0; 4]).as_bytes())
        })
    }

    // Unlike Canonical JSON proper, we **do** escape control characters
    wrapper!(write_char_escape, CharEscape);

    wrapper!(begin_array);
    wrapper!(end_array);
    wrapper!(begin_array_value, bool); // hack: this passes through the `first` argument
    wrapper!(end_array_value);

    // Here are the object methods. Because keys must be sorted, we serialize the
    // object's keys and values in memory as a `BTreeMap`, then write it all out
    // when `end_object_value` is called.

    fn begin_object<W: Write + ?Sized>(&mut self, writer: &mut W) -> Result<()> {
        CompactFormatter.begin_object(&mut self.writer(writer))?;
        self.object_stack.push(Object::default());
        Ok(())
    }

    fn end_object<W: Write + ?Sized>(&mut self, writer: &mut W) -> Result<()> {
        let object = self.object_stack.pop().ok_or_else(|| {
            Error::new(
                ErrorKind::Other,
                "serde_json called Formatter::end_object object method
                 without calling begin_object first",
            )
        })?;
        let mut writer = self.writer(writer);
        let mut first = true;

        for (key, value) in object.obj {
            CompactFormatter.begin_object_key(&mut writer, first)?;
            writer.write_all(&key)?;
            CompactFormatter.end_object_key(&mut writer)?;

            CompactFormatter.begin_object_value(&mut writer)?;
            writer.write_all(&value)?;
            CompactFormatter.end_object_value(&mut writer)?;

            first = false;
        }

        CompactFormatter.end_object(&mut writer)
    }

    fn begin_object_key<W: Write + ?Sized>(&mut self, _writer: &mut W, _first: bool) -> Result<()> {
        let mut object = self.obj_mut()?;
        object.key_done = false;
        Ok(())
    }

    fn end_object_key<W: Write + ?Sized>(&mut self, _writer: &mut W) -> Result<()> {
        let mut object = self.obj_mut()?;
        object.key_done = true;
        Ok(())
    }

    fn begin_object_value<W: Write + ?Sized>(&mut self, _writer: &mut W) -> Result<()> {
        Ok(())
    }

    fn end_object_value<W: Write + ?Sized>(&mut self, _writer: &mut W) -> Result<()> {
        let object = self.obj_mut()?;
        let key = std::mem::replace(&mut object.next_key, Vec::new());
        let value = std::mem::replace(&mut object.next_value, Vec::new());
        object.obj.insert(key, value);
        Ok(())
    }

    // This is for serde_json's `raw_value` feature, which provides a RawValue type
    // that is passed through as-is. That's not good enough for canonical JSON,
    // so we parse it and immediately write it back out... as canonical JSON.
    fn write_raw_fragment<W: Write + ?Sized>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> Result<()> {
        let mut ser = Serializer::with_formatter(self.writer(writer), Self::new());
        serde_json::from_str::<serde_json::Value>(fragment)?.serialize(&mut ser)?;
        Ok(())
    }
}
