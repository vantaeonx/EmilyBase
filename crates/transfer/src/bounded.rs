use serde::{
    Deserialize, Deserializer,
    de::{Error, SeqAccess, Visitor},
};
use std::fmt;
use std::marker::PhantomData;

pub(crate) struct List<T, const N: usize>(pub Vec<T>);
impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for List<T, N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded<T, const N: usize>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Bounded<T, N> {
            type Value = List<T, N>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded array")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                loop {
                    if values.len() == N {
                        if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                            return Err(A::Error::custom("array limit"));
                        }
                        break;
                    }
                    match sequence.next_element()? {
                        Some(value) => values.push(value),
                        None => break,
                    }
                }
                Ok(List(values))
            }
        }
        deserializer.deserialize_seq(Bounded::<T, N>(PhantomData))
    }
}
pub(crate) struct Text<const N: usize>(pub String);
impl<'de, const N: usize> Deserialize<'de> for Text<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bounded<const N: usize>;
        impl<const N: usize> Visitor<'_> for Bounded<N> {
            type Value = Text<N>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded UTF-8 text")
            }
            fn visit_str<E: Error>(self, text: &str) -> Result<Self::Value, E> {
                if text.len() > N {
                    return Err(E::custom("text limit"));
                }
                Ok(Text(text.into()))
            }
        }
        deserializer.deserialize_str(Bounded::<N>)
    }
}
