use chrono;
use image_hash::ImageHash;
use regex::Regex;
use std::collections::HashSet;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use string_cache::DefaultAtom;

pub type DateTime = chrono::DateTime<chrono::Utc>;
pub type TweetId = u64;
pub type RaidId = String;
pub type BossLevel = i16;

lazy_static! {
    static ref REGEX_BOSS_NAME: Regex = Regex::new("\
        Lv(?:l )?(?P<level>[0-9]+) .*\
    ").expect("invalid boss name regex");
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Message<'a> {
    Heartbeat,
    Tweet(&'a RaidTweet),
    TweetList(&'a [Arc<RaidTweet>]),
    BossUpdate(&'a RaidBoss),
    BossList(&'a [&'a RaidBoss]),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RaidBoss {
    pub name: BossName,
    pub level: BossLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<BossImageUrl>,
    pub language: Language,
    pub translations: HashSet<BossName>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaidBossMetadata {
    pub boss: RaidBoss,
    pub last_seen: DateTime,
    pub image_hash: Option<ImageHash>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct BossName(DefaultAtom);
impl Deref for BossName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for BossName {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.deref().fmt(f)
    }
}

impl<T> From<T> for BossName
where
    T: AsRef<str>,
{
    fn from(t: T) -> Self {
        BossName(t.as_ref().into())
    }
}

impl BossName {
    pub fn parse_level(&self) -> Option<BossLevel> {
        REGEX_BOSS_NAME.captures(self.0.as_ref()).and_then(|c| {
            c.name("level").and_then(
                |l| l.as_str().parse::<BossLevel>().ok(),
            )
        })
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct BossImageUrl(DefaultAtom);
impl Deref for BossImageUrl {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl BossImageUrl {
    #[inline]
    pub fn as_str(&self) -> &str {
        self
    }
}

impl<T> From<T> for BossImageUrl
where
    T: AsRef<str>,
{
    fn from(t: T) -> Self {
        BossImageUrl(t.as_ref().into())
    }
}

impl fmt::Display for BossImageUrl {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.deref().fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RaidTweet {
    pub tweet_id: TweetId,
    pub boss_name: BossName,
    pub raid_id: String,
    pub user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub created_at: DateTime,
    pub language: Language,
}


#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Language {
    Japanese,
    English,
    Other,
}
