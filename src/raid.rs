use chrono;
use regex::Regex;
use std::ops::Deref;
use string_cache::DefaultAtom;
use twitter_stream::message::Tweet;

pub type DateTime = chrono::DateTime<chrono::Utc>;
pub type TweetId = u64;
pub type RaidId = String;
pub type BossLevel = i16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BossName(DefaultAtom);
impl Deref for BossName {
    type Target = DefaultAtom;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BossImageUrl(DefaultAtom);
impl Deref for BossImageUrl {
    type Target = DefaultAtom;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl BossName {
    pub fn parse_level(&self) -> Option<BossLevel> {
        parse_level(self)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RaidInfo {
    pub tweet: RaidTweet,
    pub image: Option<BossImageUrl>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RaidTweet {
    pub tweet_id: TweetId,
    pub boss_name: BossName,
    pub raid_id: String,
    pub user: String,
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

    static ref REGEX_BOSS_NAME: Regex = Regex::new("\
        Lv(?:l )?(?P<level>[0-9]+) .*\
    ").expect("invalid boss name regex");
}

impl RaidInfo {
    pub fn from_tweet(mut tweet: Tweet) -> Option<RaidInfo> {
        if tweet.source != GRANBLUE_APP_SOURCE {
            return None;
        }

        let text = ::std::mem::replace(&mut tweet.text, "".into());

        parse_text(&text).map(move |parsed| {
            let user_image = if tweet.user.default_profile_image ||
                tweet.user.profile_image_url_https.contains(
                    "default_profile",
                )
            {
                None
            } else {
                Some(tweet.user.profile_image_url_https.into())
            };

            let raid_tweet = RaidTweet {
                tweet_id: tweet.id,
                boss_name: BossName(parsed.boss_name.into()),
                raid_id: parsed.raid_id.into(),
                user: tweet.user.screen_name.into(),
                user_image,
                text: parsed.text.map(Into::into),
                created_at: tweet.created_at,
                language: parsed.language,
            };

            let image = tweet.entities.media.and_then(|mut media| {
                media.pop().map(|m| BossImageUrl(m.media_url_https.into()))
            });

            RaidInfo {
                tweet: raid_tweet,
                image,
            }
        })
    }
}

fn parse_text<'a>(tweet_text: &'a str) -> Option<TweetParts<'a>> {
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

fn parse_level(name: &str) -> Option<BossLevel> {
    REGEX_BOSS_NAME.captures(name).and_then(|c| {
        c.name("level").and_then(
            |l| l.as_str().parse::<BossLevel>().ok(),
        )
    })
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
            parse_text("#GranblueHaiku http://example.com/haiku.png"),
            None
        );
    }

    #[test]
    fn parse_ignore_daily_refresh() {
        // Ignore tweets made via the daily Twitter refresh
        // https://github.com/walfie/gbf-raidfinder/issues/98
        assert_eq!(
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
            parse_text(
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
