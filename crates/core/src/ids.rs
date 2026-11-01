//! Typed ids. On the wire they are plain integers; for people and agents they print with a kind
//! prefix (`U3`, `C12`, `M450`) and parse from either form.

use serde::{Deserialize, Deserializer, Serialize};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Default)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        /// Accepts `12`, `"12"` and `"U12"`. Strings matter: JSON object keys are strings, and
        /// serde hands them over as strings inside internally tagged enums.
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct V;
                impl serde::de::Visitor<'_> for V {
                    type Value = $name;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "an id like 12 or \"{}12\"", $prefix)
                    }
                    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<$name, E> {
                        Ok($name(v))
                    }
                    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<$name, E> {
                        u64::try_from(v).map($name).map_err(|_| E::custom("ids are not negative"))
                    }
                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<$name, E> {
                        $name::parse(v).ok_or_else(|| E::custom(format!("`{v}` is not an id")))
                    }
                }
                d.deserialize_any(V)
            }
        }

        impl $name {
            /// The one-letter prefix used when the id is printed.
            pub const PREFIX: char = $prefix;

            /// Parses `"U12"`, `"u12"` or `"12"`. Returns `None` for anything else.
            pub fn parse(s: &str) -> Option<Self> {
                let s = s.trim();
                let digits = match s.chars().next() {
                    Some(c) if c.eq_ignore_ascii_case(&Self::PREFIX) => s.get(1..)?,
                    _ => s,
                };
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                digits.parse().ok().map(Self)
            }

            /// The next id after this one (saturating).
            pub fn next(self) -> Self {
                Self(self.0.saturating_add(1))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}{}", Self::PREFIX, self.0)
            }
        }
    };
}

id_type!(
    /// A person or bot in the workspace.
    UserId,
    'U'
);
id_type!(
    /// A conversation: public or private channel, direct or group message.
    ChannelId,
    'C'
);
id_type!(
    /// A message or a thread reply. Ids grow monotonically, so they also order messages.
    MessageId,
    'M'
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_display_and_parse() {
        assert_eq!(UserId(7).to_string(), "U7");
        assert_eq!(UserId::parse("U7"), Some(UserId(7)));
        assert_eq!(UserId::parse("u7"), Some(UserId(7)));
        assert_eq!(ChannelId::parse("12"), Some(ChannelId(12)));
        assert_eq!(MessageId::parse(" M450 "), Some(MessageId(450)));
    }

    #[test]
    fn ids_deserialize_from_numbers_and_strings() {
        let ids: Vec<UserId> = serde_json::from_str(r#"[3, "4", "U5"]"#).unwrap();
        assert_eq!(ids, vec![UserId(3), UserId(4), UserId(5)]);
        assert!(serde_json::from_str::<UserId>("-1").is_err());
        assert!(serde_json::from_str::<UserId>("\"nope\"").is_err());
        let m: std::collections::BTreeMap<UserId, u8> = serde_json::from_str(r#"{"7": 1}"#).unwrap();
        assert_eq!(m.get(&UserId(7)), Some(&1));
    }

    #[test]
    fn hostile_ids_are_rejected() {
        for bad in ["", "U", "C", "U-1", "U1.5", "x12", "U99999999999999999999999", "M1 2"] {
            assert_eq!(UserId::parse(bad), None, "{bad}");
        }
        assert_eq!(MessageId(u64::MAX).next(), MessageId(u64::MAX));
    }
}
