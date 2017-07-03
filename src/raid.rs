use chrono;
use regex::Regex;
use twitter_stream::message::Tweet;

pub type DateTime = chrono::DateTime<chrono::Utc>;
pub type TweetId = u64;
pub type RaidId = String;

pub struct Raid {
    pub tweet_id: TweetId,
    pub user: String,
    pub boss_name: String,
    pub raid_id: String,
    pub user_image: Option<String>,
    pub text: Option<String>,
    pub created_at: DateTime,
    pub language: Language,
}

#[derive(Clone, Debug, PartialEq)]
struct TweetParts<'a> {
    language: Language,
    text: Option<&'a str>,
    raid_id: &'a str,
    boss_name: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    Japanese,
    English,
    Other,
}

const GRANBLUE_APP_SOURCE: &'static str =
r#"<a href="http://granbluefantasy.jp/" rel="nofollow">グランブルー ファンタジー</a>"#;

lazy_static! {
    static ref REGEX_JAPANESE: Regex = Regex::new("\
        (?P<text>(?s).*)参加者募集！参戦ID：(?P<id>[0-9A-F]+)\n\
        (?P<boss>.+)\n?\
        (?P<url>.*)\
    ").expect("invalid Japanese raid tweet regex");

    static ref REGEX_ENGLISH: Regex = Regex::new("\
        (?P<text>(?s).*)I need backup!Battle ID: (?P<id>[0-9A-F]+)\n\
        (?P<boss>.+)\n?\
        (?P<url>.*)\
    ").expect("invalid English raid tweet regex");

    static ref REGEX_IMAGE_URL: Regex = Regex::new("^https?://[^ ]+$")
        .expect("invalid image URL regex");
}

impl Raid {
    fn parse<'a>(tweet_text: &'a str) -> Option<TweetParts<'a>> {
        REGEX_JAPANESE
            .captures(tweet_text)
            .map(|c| (Language::Japanese, c))
            .or_else(|| {
                REGEX_ENGLISH.captures(tweet_text).map(
                    |c| (Language::English, c),
                )
            })
            .and_then(|(lang, c)| if let (Some(text),
                                Some(id),
                                Some(boss),
                                Some(url)) =
                (c.name("text"), c.name("id"), c.name("boss"), c.name("url"))
            {
                let boss_name = boss.as_str().trim();
                let url_str = url.as_str();

                if boss_name.contains("http") ||
                    !url_str.is_empty() && !REGEX_IMAGE_URL.is_match(url_str)
                {
                    return None;
                }

                let t = text.as_str().trim();

                Some(TweetParts {
                    language: lang,
                    text: if t.is_empty() { None } else { Some(t) },
                    raid_id: id.as_str().trim(),
                    boss_name,
                })
            } else {
                None
            })
    }

    pub fn from_tweet(mut tweet: Tweet) -> Option<Raid> {
        if tweet.source != GRANBLUE_APP_SOURCE {
            return None;
        }

        let text = ::std::mem::replace(&mut tweet.text, "".into());

        Self::parse(&text).map(move |parsed| {
            let user_image = if tweet.user.default_profile_image {
                None
            } else {
                Some(tweet.user.profile_image_url_https.into())
            };

            Raid {
                tweet_id: tweet.id,
                user: tweet.user.screen_name.into(),
                boss_name: parsed.boss_name.into(),
                raid_id: parsed.raid_id.into(),
                user_image,
                text: parsed.text.map(Into::into),
                created_at: tweet.created_at,
                language: parsed.language,
            }
        })
    }
}

#[cfg(test)]
impl<'a> TweetParts<'a> {
    fn new(
        language: Language,
        text: Option<&'a str>,
        raid_id: &'a str,
        boss_name: &'a str,
    ) -> Self {
        TweetParts {
            language,
            text,
            raid_id,
            boss_name,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use super::Language::{English, Japanese};

    #[test]
    fn parse_ignore_invalid_text() {
        assert_eq!(
            Raid::parse("#GranblueHaiku http://example.com/haiku.png"),
            None
        );
    }

    #[test]
    fn parse_ignore_daily_refresh() {
        // Ignore tweets made via the daily Twitter refresh
        // https://github.com/walfie/gbf-raidfinder/issues/98
        assert_eq!(
            Raid::parse(
                "救援依頼 参加者募集！参戦ID：114514810\n\
                Lv100 ケルベロス スマホRPGは今これをやってるよ。\
                今の推しキャラはこちら！　\
                ゲーム内プロフィール→　\
                https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            ),
            None
        );
    }

    #[test]
    fn parse_ignore_another_daily_refresh() {
        // First two lines are user input
        assert_eq!(
            Raid::parse(
                "救援依頼 参加者募集！参戦ID：114514810\n\
                Lv100 ケルベロス\n\
                スマホRPGは今これをやってるよ。\
                今の推しキャラはこちら！　\
                ゲーム内プロフィール→　\
                https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            ),
            None
        );
    }

    #[test]
    fn parse_ignore_extra_space_in_image_url() {
        // First two lines are user input
        assert_eq!(
            Raid::parse(
                "救援依頼 参加者募集！参戦ID：114514810\n\
                Lv100 ケルベロス\n\
                https://t.co/5Xgohi9wlE https://t.co/Xlu7lqQ3km",
            ),
            None
        );
    }

    #[test]
    fn parse_without_extra_text() {
        assert_eq!(
            Raid::parse(
                "参加者募集！参戦ID：ABCD1234\n\
                Lv60 オオゾラッコ\n\
                http://example.com/image-that-is-ignored.png",
            ),
            Some(TweetParts::new(
                Japanese,
                None,
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            Raid::parse(
                "I need backup!Battle ID: ABCD1234\n\
                Lvl 60 Ozorotter\n\
                http://example.com/image-that-is-ignored.png",
            ),
            Some(TweetParts::new(
                English,
                None,
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn parse_without_image_url() {
        assert_eq!(
            Raid::parse(
                "Help me 参加者募集！参戦ID：ABCD1234\n\
                Lv60 オオゾラッコ",
            ),
            Some(TweetParts::new(
                Japanese,
                Some("Help me"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            Raid::parse(
                "Help me I need backup!Battle ID: ABCD1234\n\
                Lvl 60 Ozorotter",
            ),
            Some(TweetParts::new(
                English,
                Some("Help me"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn parse_with_extra_newline() {
        assert_eq!(
            Raid::parse(
                "参加者募集！参戦ID：ABCD1234\n\
                Lv60 オオゾラッコ\n",
            ),
            Some(TweetParts::new(
                Japanese,
                None,
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            Raid::parse(
                "I need backup!Battle ID: ABCD1234\n\
                Lvl 60 Ozorotter\n",
            ),
            Some(TweetParts::new(
                English,
                None,
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }


    #[test]
    fn parse_extra_text() {
        assert_eq!(
            Raid::parse(
                "Help me 参加者募集！参戦ID：ABCD1234\n\
                Lv60 オオゾラッコ\n\
                http://example.com/image-that-is-ignored.png",
            ),
            Some(TweetParts::new(
                Japanese,
                Some("Help me"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            Raid::parse(
                "Help me I need backup!Battle ID: ABCD1234\n\
                Lvl 60 Ozorotter\n\
                http://example.com/image-that-is-ignored.png",
            ),
            Some(TweetParts::new(
                English,
                Some("Help me"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }

    #[test]
    fn parse_newlines_in_extra_text() {
        assert_eq!(
            Raid::parse(
                "Hey\n\
                Newlines\n\
                Are\n\
                Cool\n\
                参加者募集！参戦ID：ABCD1234\n\
                Lv60 オオゾラッコ\n\
                http://example.com/image-that-is-ignored.png",
            ),
            Some(TweetParts::new(
                Japanese,
                Some("Hey\nNewlines\nAre\nCool"),
                "ABCD1234",
                "Lv60 オオゾラッコ",
            ))
        );

        assert_eq!(
            Raid::parse(
                "Hey\n\
                Newlines\n\
                Are\n\
                Cool\n\
                I need backup!Battle ID: ABCD1234\n\
                Lvl 60 Ozorotter\n\
                http://example.com/image-that-is-ignored.png",
            ),
            Some(TweetParts::new(
                English,
                Some("Hey\nNewlines\nAre\nCool"),
                "ABCD1234",
                "Lvl 60 Ozorotter",
            ))
        );
    }
}
