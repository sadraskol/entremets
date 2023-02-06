use std::fmt::{Display, Error, Formatter};

pub fn intersperse<T: Display>(f: &mut Formatter, values: &[T], sep: &str) -> Result<(), Error> {
    let mut iter = values.iter().peekable();
    while let Some(value) = iter.next() {
        Display::fmt(&value, f)?;
        if iter.peek().is_some() {
            f.write_str(&(sep.to_owned() + " "))?;
        }
    }
    Ok(())
}
