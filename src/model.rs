use kv::{Error, Raw};
use serde::{Deserialize, Serialize};
use std::ops::Add;
use std::time::{Duration, SystemTime};

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct TokenId([u8; 8]);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Email(pub String);

impl AsRef<[u8]> for TokenId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl<'a> kv::Key<'a> for TokenId {
    fn from_raw_key(x: &Raw) -> Result<Self, Error> {
        let mut dst = Self([0; 8]);
        dst.0[..8].clone_from_slice(&x.as_ref()[..8]);
        Ok(dst)
    }
}
impl From<u64> for TokenId {
    fn from(v: u64) -> Self {
        Self(v.to_be_bytes())
    }
}

impl AsRef<[u8]> for Email {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}
impl<'a> kv::Key<'a> for Email {
    fn from_raw_key(r: &'a Raw) -> Result<Self, Error> {
        Ok(Self(std::str::from_utf8(r.as_ref())?.to_string()))
    }
}

macro_rules! impl_value {
    ($name:ident) => {
        impl kv::Value for $name {
            fn to_raw_value(&self) -> Result<Raw, Error> {
                let bc = bincode::serialize(self)?;
                Ok(bc.into())
            }

            fn from_raw_value(r: Raw) -> Result<Self, Error> {
                let de = bincode::deserialize(r.as_ref())?;
                Ok(de)
            }
        }
    };
}

#[derive(Serialize, Deserialize)]
pub struct UpdateQueueKey([u8; 16]);

impl AsRef<[u8]> for UpdateQueueKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl<'a> kv::Key<'a> for UpdateQueueKey {
    fn from_raw_key(x: &Raw) -> Result<Self, Error> {
        let mut dst = Self([0; 16]);
        dst.0[..16].clone_from_slice(&x.as_ref()[..16]);
        Ok(dst)
    }
}

impl From<(SystemTime, TokenId)> for UpdateQueueKey {
    fn from((t, id): (SystemTime, TokenId)) -> Self {
        let t: u64 = t
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("SystemTime not representable as UNIX time")
            .as_millis()
            .try_into()
            .expect("SystemTime too far into the future");

        let mut r = [0u8; 16];
        r[..8].copy_from_slice(&t.to_be_bytes());
        r[8..].copy_from_slice(&id.0);

        Self(r)
    }
}

impl From<UpdateQueueKey> for (SystemTime, TokenId) {
    fn from(k: UpdateQueueKey) -> Self {
        let mut time = [0u8; 8];
        time.copy_from_slice(&k.0[..8]);

        let time = u64::from_be_bytes(time);
        let time = SystemTime::UNIX_EPOCH.add(Duration::from_millis(time));

        let mut key = [0u8; 8];
        key.copy_from_slice(&k.0[8..]);

        let key = TokenId(key);

        (time, key)
    }
}

#[derive(Serialize, Deserialize)]
pub struct User {
    pub email: Email,
    pub tokens: Vec<TokenId>,
}
impl_value!(User);

#[derive(Serialize, Deserialize)]
pub struct Token {
    pub owner: Email,
    pub moodle_session: String,
    pub csrf_session: Option<String>,
    #[serde(with = "serde_millis")]
    pub last_updated: SystemTime,
    #[serde(with = "serde_millis")]
    pub added: SystemTime,
}
impl_value!(Token);

#[derive(Serialize, Deserialize)]
pub struct UpdateQueueItem {
    pub token: TokenId,
}
impl_value!(UpdateQueueItem);
