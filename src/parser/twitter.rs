use super::super::DateTime;
use serde;
use serde::de::{Deserializer, SeqAccess, Visitor};
use std::fmt;

// Twitter API struct definitions with only the fields we care about
#[derive(Deserialize)]
pub(crate) struct Tweet<'a> {
    #[serde(deserialize_with = "deserialize_datetime")]
    pub(crate) created_at: DateTime,
    pub(crate) text: &'a str,
    pub(crate) source: &'a str,
    pub(crate) user: User<'a>,
    pub(crate) entities: Entities<'a>,
}

#[derive(Deserialize)]
pub(crate) struct Entities<'a> {
    // We only care about the first item in the `media` array, if it's
    // present. To avoid needing to allocate a `Vec` for the `media` array,
    // we use use a custom deserializer to get the first element.
    #[serde(borrow, default, deserialize_with = "deserialize_media")]
    pub(crate) media: Option<Media<'a>>,
}

#[derive(Deserialize)]
pub(crate) struct Media<'a> {
    pub(crate) media_url_https: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct User<'a> {
    pub(crate) screen_name: &'a str,
    pub(crate) default_profile_image: bool,
    pub(crate) profile_image_url_https: &'a str,
}

// Based heavily on the deserializer from the `twitter-stream-message` crate
fn deserialize_datetime<'de, D>(deserializer: D) -> Result<DateTime, D::Error>
where
    D: Deserializer<'de>,
{
    struct DateTimeVisitor;

    impl<'de> Visitor<'de> for DateTimeVisitor {
        type Value = DateTime;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a valid date time string")
        }

        fn visit_str<E>(self, s: &str) -> Result<DateTime, E>
        where
            E: serde::de::Error,
        {
            pub fn parse_datetime(s: &str) -> ::chrono::format::ParseResult<DateTime> {
                use chrono::Utc;
                use chrono::format::{self, Fixed, Item, Numeric, Pad, Parsed};

                // "%a %b %e %H:%M:%S %z %Y"
                const ITEMS: &'static [Item<'static>] = &[
                    Item::Fixed(Fixed::ShortWeekdayName),
                    Item::Space(" "),
                    Item::Fixed(Fixed::ShortMonthName),
                    Item::Space(" "),
                    Item::Numeric(Numeric::Day, Pad::Space),
                    Item::Space(" "),
                    Item::Numeric(Numeric::Hour, Pad::Zero),
                    Item::Literal(":"),
                    Item::Numeric(Numeric::Minute, Pad::Zero),
                    Item::Literal(":"),
                    Item::Numeric(Numeric::Second, Pad::Zero),
                    Item::Space(" "),
                    Item::Fixed(Fixed::TimezoneOffset),
                    Item::Space(" "),
                    Item::Numeric(Numeric::Year, Pad::Zero),
                ];

                let mut parsed = Parsed::new();
                format::parse(&mut parsed, s, ITEMS.iter().cloned())?;
                parsed.to_datetime_with_timezone(&Utc)
            }

            parse_datetime(s).map_err(|e| E::custom(e.to_string()))
        }
    }

    deserializer.deserialize_str(DateTimeVisitor)
}

fn deserialize_media<'de, D>(deserializer: D) -> Result<Option<Media<'de>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MediaVisitor;

    impl<'de> Visitor<'de> for MediaVisitor {
        type Value = Option<Media<'de>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an array of media objects")
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: SeqAccess<'de>,
        {
            let mut out = None;

            while let Ok(Some(item)) = seq.next_element() {
                out = item;
            }

            Ok(out)
        }
    }

    deserializer.deserialize_seq(MediaVisitor)
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json;

    #[test]
    fn parse_tweet() {
        let json = r#"{
            "created_at": "Thu Apr 06 15:24:15 +0000 2017",
            "source": "petronel",
            "text": "Hello world!",
            "entities": {
                "media": [
                    { "media_url_https": "https://example.com/media.jpg" }
                ]
            },
            "user": {
                "screen_name": "walfieee",
                "default_profile_image": false,
                "profile_image_url_https": "https://example.com/icon.png"
            }
        }"#;

        let tweet: Tweet = serde_json::from_str(json).unwrap();
        assert_eq!(tweet.source, "petronel");
        assert_eq!(tweet.created_at, Utc.ymd(2017, 4, 6).and_hms(15, 24, 15));
        assert_eq!(tweet.text, "Hello world!");
        assert_eq!(tweet.user.screen_name, "walfieee");
        assert_eq!(tweet.user.default_profile_image, false);
        assert_eq!(
            tweet.user.profile_image_url_https,
            "https://example.com/icon.png"
        );
        assert_eq!(
            tweet.entities.media.unwrap().media_url_https,
            "https://example.com/media.jpg"
        );
    }

    #[test]
    fn parse_tweet_with_multiple_media_items() {
        // Take the last `media` item in the array
        let json = r#"{
            "created_at": "Thu Apr 06 15:24:15 +0000 2017",
            "source": "",
            "text": "",
            "entities": {
                "media": [
                    { "media_url_https": "https://example.com/media1.jpg" },
                    { "media_url_https": "https://example.com/media2.jpg" }
                ]
            },
            "user": {
                "screen_name": "",
                "default_profile_image": false,
                "profile_image_url_https": ""
            }
        }"#;

        let tweet: Tweet = serde_json::from_str(json).unwrap();
        assert_eq!(
            tweet.entities.media.unwrap().media_url_https,
            "https://example.com/media2.jpg"
        );
    }

    #[test]
    fn parse_tweet_with_empty_media_array() {
        let json = r#"{
            "created_at": "Thu Apr 06 15:24:15 +0000 2017",
            "source": "",
            "text": "",
            "entities": {
                "media": []
            },
            "user": {
                "screen_name": "",
                "default_profile_image": false,
                "profile_image_url_https": ""
            }
        }"#;

        let tweet: Tweet = serde_json::from_str(json).unwrap();
        assert!(tweet.entities.media.is_none());
    }

    #[test]
    fn parse_tweet_with_no_media() {
        let json = r#"{
            "created_at": "Thu Apr 06 15:24:15 +0000 2017",
            "source": "",
            "text": "",
            "entities": {},
            "user": {
                "screen_name": "",
                "default_profile_image": false,
                "profile_image_url_https": ""
            }
        }"#;

        let tweet: Tweet = serde_json::from_str(json).unwrap();
        assert!(tweet.entities.media.is_none());
    }
}
