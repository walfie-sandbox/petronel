use error::*;
use futures::{Async, Future, Poll, Stream};
use futures::future::FlattenStream;
use hyper;
use model::{BossImageUrl, Language, RaidTweet};
use regex::Regex;
use tokio_core::reactor::Handle;
use twitter_stream::{FutureTwitterStream, Token, TwitterStreamBuilder};
use twitter_stream::message::StreamMessage;
use twitter_stream::message::Tweet;

const GRANBLUE_APP_SOURCE: &'static str =
r#"<a href="http://granbluefantasy.jp/" rel="nofollow">グランブルー ファンタジー</a>"#;

lazy_static! {
    static ref REGEX_JAPANESE: Regex = Regex::new("\
        (?P<text>(?s).*)(?P<id>[0-9A-F]{8}) :参戦ID\n\
        参加者募集！\n\
        (?P<boss>.+)\n?\
        (?P<url>.*)\
    ").expect("invalid Japanese raid tweet regex");

    static ref REGEX_ENGLISH: Regex = Regex::new("\
        (?P<text>(?s).*)(?P<id>[0-9A-F]{8}) :Battle ID\n\
        I need backup!\n\
        (?P<boss>.+)\n?\
        (?P<url>.*)\
    ").expect("invalid English raid tweet regex");

    static ref REGEX_IMAGE_URL: Regex = Regex::new("^https?://[^ ]+$")
        .expect("invalid image URL regex");
}

#[must_use = "streams do nothing unless polled"]
pub struct RaidInfoStream(FlattenStream<FutureTwitterStream>);

// TODO: Add version that reconnects on disconnect/error
impl RaidInfoStream {
    fn track() -> &'static str {
        "参加者募集！,:参戦ID,I need backup!,:Battle ID"
    }

    pub fn with_client<C, B>(hyper_client: &hyper::Client<C, B>, token: &Token) -> Self
    where
        C: hyper::client::Connect,
        B: From<Vec<u8>> + Stream<Error = hyper::Error> + 'static,
        B::Item: AsRef<[u8]>,
    {
        let stream = TwitterStreamBuilder::filter(token)
            .client(&hyper_client)
            .user_agent(Some("petronel")) // TODO: Make this configurable?
            .timeout(None)
            .track(Some(Self::track()))
            .listen()
            .flatten_stream();

        RaidInfoStream(stream)
    }

    // TODO: Clean up duplicated code
    pub fn with_handle(handle: &Handle, token: &Token) -> Self {
        let stream = TwitterStreamBuilder::filter(token)
            .handle(handle)
            .user_agent(Some("petronel")) // TODO: Make this configurable?
            .timeout(None)
            .track(Some(&Self::track()))
            .listen()
            .flatten_stream();

        RaidInfoStream(stream)
    }
}

impl Stream for RaidInfoStream {
    type Item = RaidInfo;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let polled = self.0.poll().chain_err(|| ErrorKind::Twitter);
            if let Some(json) = try_ready!(polled) {
                let msg = StreamMessage::from_str(json.as_ref())
                    .chain_err(|| ErrorKind::Json(json.to_string()))?;

                if let StreamMessage::Tweet(tweet) = msg {
                    if let Some(raid_info) = RaidInfo::from_tweet(*tweet) {
                        return Ok(Async::Ready(Some(raid_info)));
                    }
                }
            } else {
                return Ok(Async::Ready(None));
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TweetParts<'a> {
    language: Language,
    text: Option<&'a str>,
    raid_id: &'a str,
    boss_name: &'a str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RaidInfo {
    pub tweet: RaidTweet,
    pub image: Option<BossImageUrl>,
}

impl RaidInfo {
    pub fn from_tweet(mut tweet: Tweet) -> Option<RaidInfo> {
        if tweet.source != GRANBLUE_APP_SOURCE {
            return None;
        }

        let text = ::std::mem::replace(&mut tweet.text, "".into());

        parse_text(&text).map(move |parsed| {
            let user_image = if tweet.user.default_profile_image
                || tweet
                    .user
                    .profile_image_url_https
                    .contains("default_profile")
            {
                None
            } else {
                Some(tweet.user.profile_image_url_https.into())
            };

            let raid_tweet = RaidTweet {
                tweet_id: tweet.id,
                boss_name: parsed.boss_name.into(),
                raid_id: parsed.raid_id.into(),
                user: tweet.user.screen_name.into(),
                user_image,
                text: parsed.text.map(Into::into),
                created_at: tweet.created_at,
                language: parsed.language,
            };

            let image = tweet
                .entities
                .media
                .and_then(|mut media| media.pop().map(|m| m.media_url_https.into()));

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
            REGEX_ENGLISH
                .captures(tweet_text)
                .map(|c| (Language::English, c))
        })
        .and_then(|(lang, c)| {
            if let (Some(text), Some(id), Some(boss), Some(url)) =
                (c.name("text"), c.name("id"), c.name("boss"), c.name("url"))
            {
                let boss_name = boss.as_str().trim();
                let url_str = url.as_str();

                if boss_name.contains("http")
                    || !url_str.is_empty() && !REGEX_IMAGE_URL.is_match(url_str)
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
            }
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
                "ABCD1234 :参戦ID\n\
                 参加者募集！\n\
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
                "ABCD1234 :Battle ID\n\
                 I need backup!\n\
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
                "Help me ABCD1234 :参戦ID\n\
                 参加者募集！\n\
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
                "Help me ABCD1234 :Battle ID\n\
                 I need backup!\n\
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
                "ABCD1234 :参戦ID\n\
                 参加者募集！\n\
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
                "ABCD1234 :Battle ID\n\
                 I need backup!\n\
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
                "Help me ABCD1234 :参戦ID\n\
                 参加者募集！\n\
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
                "Help me ABCD1234 :Battle ID\n\
                 I need backup!\n\
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
                 ABCD1234 :参戦ID\n\
                 参加者募集！\n\
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
                 ABCD1234 :Battle ID\n\
                 I need backup!\n\
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
