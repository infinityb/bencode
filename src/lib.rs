use std::io::{self, Write};
use std::iter::Peekable;
use std::collections::BTreeMap;


#[derive(PartialEq, Eq, Debug)]
pub enum Bencode {
    Integer(Vec<u8>),
    Bytes(Vec<u8>),
    Array(Vec<Bencode>),
    Object(BTreeMap<Vec<u8>, Bencode>),
}

type BencodeResult<T> = Result<T, ParseError>;

pub enum ParseError {
    Truncated,
    InvalidCharacter,
    InvalidLength,
    OutOfOrderKey,
}

fn is_digit(val: u8) -> bool {
    b'0' <= val && val <= b'9'
}

fn bdecode_extract_integer<I>(stream: &mut Peekable<I>)
    -> BencodeResult<Vec<u8>>
    where
        I: Iterator<Item=u8> {

    let mut buf = Vec::new();
    loop {
        match stream.peek() {
            Some(&val) if is_digit(val) => buf.push(stream.next().unwrap()),
            Some(_) => return Ok(buf),
            None => return Err(ParseError::Truncated)
        }
    }
}

fn bdecode_integer<I>(stream: &mut Peekable<I>) -> BencodeResult<Vec<u8>>
    where
        I: Iterator<Item=u8> {

    let output = match stream.next() {
        Some(b'i') => try!(bdecode_extract_integer(stream)),
        Some(_) => return Err(ParseError::InvalidCharacter),
        None => return Err(ParseError::Truncated)
    };
    match stream.next() {
        Some(b'e') => Ok(output),
        Some(_) => Err(ParseError::Truncated),
        None => Err(ParseError::Truncated)
    }
}

fn bdecode_bytea<I>(stream: &mut Peekable<I>) -> BencodeResult<Vec<u8>>
    where
        I: Iterator<Item=u8> {

    let intbuf = try!(bdecode_extract_integer(stream));
    let length = std::str::from_utf8(&intbuf[..]).ok()
        .expect("bdecode_extract_integer failed to hold invariant")
        .parse::<usize>();

    let length = match length {
        Ok(value) => value,
        Err(_) => return Err(ParseError::InvalidLength),
    };

    match stream.next() {
        Some(b':') => Ok(stream.take(length).collect()),
        Some(_) => return Err(ParseError::InvalidCharacter),
        None => return Err(ParseError::Truncated)
    }
}

fn bdecode_list<I>(stream: &mut Peekable<I>) -> BencodeResult<Vec<Bencode>>
    where
        I: Iterator<Item=u8> {

    let mut output = Vec::new();
    assert_eq!(Some(b'l'), stream.next());

    loop {
        match stream.peek() {
            Some(&b'e') => {
                stream.next().expect("expected b'e'");
                return Ok(output);
            },
            Some(_) => output.push(try!(bdecode(stream))),
            None => return Err(ParseError::Truncated)
        }
    }
}

fn bdecode_dict<I>(stream: &mut Peekable<I>)
    -> BencodeResult<BTreeMap<Vec<u8>, Bencode>>
    where
        I: Iterator<Item=u8> {

    // Key order checking. Elide these checks in the future?
    let mut prev_key = Vec::new();

    let mut output = BTreeMap::new();
    assert_eq!(Some(b'd'), stream.next());

    loop {
        match stream.peek() {
            Some(&b'e') => return Ok(output),
            Some(_) => (),
            None => return Err(ParseError::Truncated),
        }
        let key = try!(bdecode_bytea(stream));
        if key < prev_key {
            return Err(ParseError::OutOfOrderKey);
        }
        prev_key.clear();
        prev_key.extend(key.iter().cloned());

        let value = try!(bdecode(stream));
        output.insert(key, value);
    }
}


fn iter_bdecode<I>(stream: &mut Peekable<I>) -> Result<Bencode, ParseError>
    where
        I: Iterator<Item=u8> {

    use Bencode::{Integer, Array, Object, Bytes};
    match stream.peek() {
        Some(&b'i') => Ok(Integer(try!(bdecode_integer(stream)))),
        Some(&b'l') => Ok(Array(try!(bdecode_list(stream)))),
        Some(&b'd') => Ok(Object(try!(bdecode_dict(stream)))),
        Some(&val) if is_digit(val) => Ok(Bytes(try!(bdecode_bytea(stream)))),
        _ => Err(ParseError::InvalidCharacter),
    }
}

fn bencode_bytea<W>(bytea: &[u8], writer: &mut W) -> Result<(), io::Error>
    where
        W: Write {

    try!(write!(writer, "{}:", bytea.len()));
    try!(writer.write_all(bytea));
    Ok(())
}

pub fn bencode<W>(document: &Bencode, writer: &mut W) -> Result<(), io::Error>
    where
        W: Write {

    match document {
        &Bencode::Integer(ref buf) => {
            try!(writer.write_all(b"i"));
            try!(writer.write_all(buf));
            try!(writer.write_all(b"e"));
        },
        &Bencode::Bytes(ref buf) => try!(bencode_bytea(buf, writer)),
        &Bencode::Array(ref items) => {
            try!(writer.write_all(b"l"));
            for item in items.iter() {
                try!(bencode(item, writer));
            }
            try!(writer.write_all(b"e"));
        },
        &Bencode::Object(ref map) => {
            try!(writer.write_all(b"d"));
            for (key, value) in map.iter() {
                try!(bencode_bytea(key, writer));
                try!(bencode(value, writer));
            }
            try!(writer.write_all(b"e"));
        },
    };
    Ok(())
}

#[test]
fn it_works() {
    let document = b"d1:a3:eh?1:bl3:beeee";

    let mut peekable = document.iter().cloned().peekable();
    let result = bdecode(&mut peekable).ok().expect("failed to parse");

    let obj = match result {
        Bencode::Object(ref obj) => obj,
        _ => panic!("Must be an Bencode::Object"),
    };

    assert_eq!(
        obj.get(b"a" as &[u8]),
        Some(&Bencode::Bytes(b"eh?".to_vec())));

    assert_eq!(
        obj.get(b"b" as &[u8]),
        Some(&Bencode::Array(vec![
            Bencode::Bytes(b"bee".to_vec())
        ])));

    let mut reserialized = Vec::new();
    bencode(&result, &mut reserialized).ok().expect("failed to serialize");
    assert_eq!(document, &reserialized[..]);
}
