mod twitter;

use self::twitter::Tweet;
use super::{Language, Parser, Raid, RaidWithBossImage};
use regex::Regex;
use serde_json;
use std::marker::PhantomData;

macro_rules! try_opt {
    ($e:expr) => (if let Some(v) = $e { v } else { return None; })
}

const GRANBLUE_APP_SOURCE: &'static str =
    r#"<a href="http://granbluefantasy.jp/" rel="nofollow">グランブルー ファンタジー</a>"#;

pub struct TweetJsonParser<T> {
    regex_jp: Regex,
    regex_en: Regex,
    regex_image_url: Regex,
    input_type: PhantomData<T>,
}

struct ParsedTweet<'a> {
    language: Language,
    text: Option<&'a str>,
    raid_id: &'a str,
    boss_name: &'a str,
}

impl<T> TweetJsonParser<T>
where
    T: AsRef<str>,
{
    pub fn new() -> Self {
        let regex_jp = Regex::new(
            "(?P<text>(?s).*)(?P<id>[0-9A-F]{8}) :参戦ID\n\
             参加者募集！\n\
             (?P<boss>.+)\n?\
             (?P<url>.*)",
        ).unwrap();

        let regex_en = Regex::new(
            "(?P<text>(?s).*)(?P<id>[0-9A-F]{8}) :Battle ID\n\
             I need backup!\n\
             (?P<boss>.+)\n?\
             (?P<url>.*)",
        ).unwrap();

        let regex_image_url = Regex::new("^https?://[^ ]+$").unwrap();

        TweetJsonParser {
            regex_jp,
            regex_en,
            regex_image_url,
            input_type: PhantomData,
        }
    }

    fn parse_text<'a>(&self, tweet_text: &'a str) -> Option<ParsedTweet<'a>> {
        let (language, c) = try_opt!(
            self.regex_jp
                .captures(tweet_text)
                .map(|c| (Language::Japanese, c))
                .or_else(|| self.regex_en
                    .captures(tweet_text)
                    .map(|c| (Language::English, c)))
        );

        if let (Some(text), Some(id), Some(boss), Some(url)) =
            (c.name("text"), c.name("id"), c.name("boss"), c.name("url"))
        {
            let boss_name = boss.as_str().trim();
            let url_str = url.as_str();

            if boss_name.contains("http")
                || !url_str.is_empty() && !self.regex_image_url.is_match(url_str)
            {
                return None;
            }

            let t = text.as_str().trim();

            Some(ParsedTweet {
                language,
                text: if t.is_empty() { None } else { Some(t) },
                raid_id: id.as_str().trim(),
                boss_name,
            })
        } else {
            None
        }
    }
}

impl<T> Parser<T> for TweetJsonParser<T>
where
    T: AsRef<str>,
{
    fn parse<'a>(&mut self, input: &'a T) -> Option<RaidWithBossImage<'a>> {
        let tweet: Tweet = try_opt!(serde_json::from_str(input.as_ref()).ok());

        if tweet.source != GRANBLUE_APP_SOURCE {
            return None;
        }

        let parsed = try_opt!(self.parse_text(tweet.text));

        let user_image = if tweet.user.default_profile_image
            || tweet
                .user
                .profile_image_url_https
                .contains("default_profile")
        {
            None
        } else {
            Some(tweet.user.profile_image_url_https)
        };

        let image = tweet.entities.media.map(|m| m.media_url_https);

        Some(RaidWithBossImage {
            image,
            raid: Raid {
                id: parsed.raid_id,
                boss: parsed.boss_name,
                text: parsed.text,
                timestamp: 0, // TODO
                user: tweet.user.screen_name,
                user_image,
            },
        })
    }
}
