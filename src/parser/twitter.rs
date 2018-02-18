use serde::de::{Deserializer, SeqAccess, Visitor};
use serde_json;
use std::fmt;

// Twitter API struct definitions with only the fields we care about
#[derive(Deserialize)]
pub(crate) struct Tweet<'a> {
    pub(crate) created_at: &'a str,
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

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: SeqAccess<'de>,
        {
            Ok(seq.next_element().ok().and_then(|v| v))
        }
    }

    deserializer.deserialize_seq(MediaVisitor)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_tweet() {
        let json = r#"{
            "created_at": "Thu Apr 06 15:24:15 +0000 2017",
            "source": "petronel",
            "text": "Hello world!",
            "user": {
                "screen_name": "walfieee",
                "default_profile_image": false,
                "profile_image_url_https": "https://example.com/icon.png"
            },
            "entities": {
                "media": [
                    { "media_url_https": "https://example.com/media.jpg" }
                ]
            }
        }"#;

        let tweet: Tweet = serde_json::from_str(json).unwrap();
        assert_eq!(tweet.source, "petronel");
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
    fn parse_tweet_with_empty_media_array() {
        let json = r#"{
            "created_at": "",
            "source": "",
            "text": "",
            "user": {
                "screen_name": "",
                "default_profile_image": false,
                "profile_image_url_https": ""
            },
            "entities": {
                "media": []
            }
        }"#;

        let tweet: Tweet = serde_json::from_str(json).unwrap();
        assert!(tweet.entities.media.is_none());
    }

    #[test]
    fn parse_tweet_with_no_media() {
        let json = r#"{
            "created_at": "",
            "source": "",
            "text": "",
            "user": {
                "screen_name": "",
                "default_profile_image": false,
                "profile_image_url_https": ""
            },
            "entities": {}
        }"#;

        let tweet: Tweet = serde_json::from_str(json).unwrap();
        assert!(tweet.entities.media.is_none());
    }
}
