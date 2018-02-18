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

#[derive(Debug)]
pub struct TweetJsonParser<T> {
    regex_jp: Regex,
    regex_en: Regex,
    regex_image_url: Regex,
    input_type: PhantomData<T>,
}

#[derive(Debug, PartialEq)]
struct ParsedTweet<'a> {
    language: Language,
    text: Option<&'a str>,
    raid_id: &'a str,
    boss_name: &'a str,
}

#[cfg(test)]
impl<'a> ParsedTweet<'a> {
    fn new(
        language: Language,
        text: Option<&'a str>,
        raid_id: &'a str,
        boss_name: &'a str,
    ) -> Self {
        ParsedTweet {
            language,
            text,
            raid_id,
            boss_name,
        }
    }
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
                timestamp: tweet.created_at,
                user: tweet.user.screen_name,
                user_image,
            },
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use super::Language::{English, Japanese};

    fn check(input: &str, expected: Option<ParsedTweet>) {
        assert_eq!(
            TweetJsonParser::<&'static str>::new().parse_text(&input),
            expected
        )
    }

    #[test]
    fn parse_ignore_invalid_text() {
        check("#GranblueHaiku http://example.com/haiku.png", None);
    }

    #[test]
    fn parse_ignore_daily_refresh() {
        // Ignore tweets made via the daily Twitter refresh
        // https://github.com/walfie/gbf-raidfinder/issues/98
        check(
            "救援依頼 参加者募集！ 114514810 ：参戦ID\n\
             Lv100 ケルベロス スマホRPGは今これをやってるよ。\
             今の推しキャラはこちら！　\
             ゲーム内プロフィール→　\
             https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            None,
        );
    }

    #[test]
    fn parse_ignore_another_daily_refresh() {
        // First two lines are user input
        check(
            "救援依頼 参加者募集！ 114514810 ：参戦ID\n\
             Lv100 ケルベロス\n\
             スマホRPGは今これをやってるよ。\
             今の推しキャラはこちら！　\
             ゲーム内プロフィール→　\
             https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            None,
        );
    }

    #[test]
    fn parse_ignore_extra_space_in_image_url() {
        // First two lines are user input
        check(
            "救援依頼 参加者募集！ 114514810 ：参戦ID\n\
             Lv100 ケルベロス\n\
             https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            None,
        );
    }

    #[test]
    fn parse_without_extra_text() {
        check(
            "ABCD1234 :参戦ID\n\
             参加者募集！\n\
             Lv60 オオゾラッコ\n\
             http://example.com/image-that-is-ignored.png",
            Some(ParsedTweet::new(
                Japanese,
                None,
                "ABCD1234",
                "Lv60 オオゾラッコ",
            )),
        );

        check(
            "ABCD1234 :Battle ID\n\
             I need backup!\n\
             Lvl 60 Ozorotter\n\
             http://example.com/image-that-is-ignored.png",
            Some(ParsedTweet::new(
                English,
                None,
                "ABCD1234",
                "Lvl 60 Ozorotter",
            )),
        );
    }

    #[test]
    fn parse_without_image_url() {
        check(
            "Help me ABCD1234 :参戦ID\n\
             参加者募集！\n\
             Lv60 オオゾラッコ",
            Some(ParsedTweet::new(
                Japanese,
                Some("Help me"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            )),
        );

        check(
            "Help me ABCD1234 :Battle ID\n\
             I need backup!\n\
             Lvl 60 Ozorotter",
            Some(ParsedTweet::new(
                English,
                Some("Help me"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            )),
        );
    }

    #[test]
    fn parse_with_extra_newline() {
        check(
            "ABCD1234 :参戦ID\n\
             参加者募集！\n\
             Lv60 オオゾラッコ\n",
            Some(ParsedTweet::new(
                Japanese,
                None,
                "ABCD1234",
                "Lv60 オオゾラッコ",
            )),
        );

        check(
            "ABCD1234 :Battle ID\n\
             I need backup!\n\
             Lvl 60 Ozorotter\n",
            Some(ParsedTweet::new(
                English,
                None,
                "ABCD1234",
                "Lvl 60 Ozorotter",
            )),
        );
    }

    #[test]
    fn parse_extra_text() {
        check(
            "Help me ABCD1234 :参戦ID\n\
             参加者募集！\n\
             Lv60 オオゾラッコ\n\
             http://example.com/image-that-is-ignored.png",
            Some(ParsedTweet::new(
                Japanese,
                Some("Help me"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            )),
        );

        check(
            "Help me ABCD1234 :Battle ID\n\
             I need backup!\n\
             Lvl 60 Ozorotter\n\
             http://example.com/image-that-is-ignored.png",
            Some(ParsedTweet::new(
                English,
                Some("Help me"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            )),
        );
    }

    #[test]
    fn parse_newlines_in_extra_text() {
        check(
            "Hey\n\
             Newlines\n\
             Are\n\
             Cool\n\
             ABCD1234 :参戦ID\n\
             参加者募集！\n\
             Lv60 オオゾラッコ\n\
             http://example.com/image-that-is-ignored.png",
            Some(ParsedTweet::new(
                Japanese,
                Some("Hey\nNewlines\nAre\nCool"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            )),
        );

        check(
            "Hey\n\
             Newlines\n\
             Are\n\
             Cool\n\
             ABCD1234 :Battle ID\n\
             I need backup!\n\
             Lvl 60 Ozorotter\n\
             http://example.com/image-that-is-ignored.png",
            Some(ParsedTweet::new(
                English,
                Some("Hey\nNewlines\nAre\nCool"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            )),
        );
    }
}
