use std::fmt::Display;
use std::str::FromStr;

use chrono::prelude::*;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::Regex;

use super::error::EhError;
use crate::database::GalleryEntity;

// 画廊地址，格式为 https://exhentai.org/g/2549143/16b1b7bab0/
#[derive(Debug, Clone, PartialEq)]
pub struct EhGalleryUrl {
    id: i32,
    token: String,
    cover: usize,
}

impl EhGalleryUrl {
    /// 画廊 URL
    pub fn url(&self) -> String {
        format!("https://exhentai.org/g/{}/{}/", self.id, self.token)
    }

    /// 画廊 ID
    pub fn id(&self) -> i32 {
        self.id
    }

    /// 画廊 token
    pub fn token(&self) -> &str {
        &self.token
    }

    /// 封面是第几张
    pub fn cover(&self) -> usize {
        self.cover
    }
}

impl FromStr for EhGalleryUrl {
    type Err = EhError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        static RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"https://e.hentai.org/g/(?P<id>\d+)/(?P<token>[^/]+)/?(?P<cover>#\d+)?")
                .unwrap()
        });
        let captures = RE.captures(s).ok_or_else(|| EhError::InvalidURL(s.to_owned()))?;
        let token = captures.name("token").unwrap().as_str().to_owned();
        let id = captures.name("id").and_then(|s| s.as_str().parse().ok()).unwrap();
        let cover = captures.name("cover").and_then(|s| s.as_str()[1..].parse().ok()).unwrap_or_default();

        Ok(Self { id, token, cover })
    }
}

impl Display for EhGalleryUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url())
    }
}

/// 画廊页面地址，格式为 https://exhentai.org/s/03af734602/1932743-1
#[derive(Debug, Clone, PartialEq)]
pub struct EhPageUrl {
    hash: String,
    gallery_id: i32,
    page: i32,
    nl: Option<String>,
}

impl EhPageUrl {
    pub fn url(&self) -> String {
        match &self.nl {
            None => {
                format!("https://exhentai.org/s/{}/{}-{}", self.hash, self.gallery_id, self.page)
            }
            Some(nl) => format!(
                "https://exhentai.org/s/{}/{}-{}?nl={}",
                self.hash, self.gallery_id, self.page, nl
            ),
        }
    }

    /// 页面哈希，实际上就是图片哈希的前十位
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// 画廊 ID
    pub fn gallery_id(&self) -> i32 {
        self.gallery_id
    }

    /// 页码
    pub fn page(&self) -> i32 {
        self.page
    }

    pub fn with_nl(&self, nl: &str) -> Self {
        EhPageUrl {
            hash: self.hash.clone(),
            gallery_id: self.gallery_id,
            page: self.page,
            nl: Some(nl.to_owned()),
        }
    }
}

impl FromStr for EhPageUrl {
    type Err = EhError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        static RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"https://e.hentai.org/s/(?P<hash>.+)/(?P<id>\d+)-(?P<page>\d+)").unwrap()
        });

        let captures = RE.captures(s).ok_or_else(|| EhError::InvalidURL(s.to_owned()))?;
        let hash = captures.name("hash").unwrap().as_str().to_owned();
        let gallery_id = captures.name("id").and_then(|s| s.as_str().parse().ok()).unwrap();
        let page = captures.name("page").and_then(|s| s.as_str().parse().ok()).unwrap();

        Ok(Self { hash: hash.to_owned(), gallery_id, page, nl: None })
    }
}

impl Display for EhPageUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url())
    }
}

#[derive(Debug, Clone)]
pub struct EhGallery {
    pub url: EhGalleryUrl,
    pub title: String,
    pub title_jp: Option<String>,
    pub tags: IndexMap<String, Vec<String>>,
    pub favorite: i32,
    pub parent: Option<EhGalleryUrl>,
    pub pages: Vec<EhPageUrl>,
    pub posted: NaiveDateTime,
    pub cover: usize,
    pub album_id: Option<String>, // 新增字段
}

pub trait GalleryInfo {
    fn url(&self) -> EhGalleryUrl;
    fn title(&self) -> String;
    fn title_jp(&self) -> String;
    fn tags(&self) -> &IndexMap<String, Vec<String>>;
    fn pages(&self) -> usize;
    fn cover(&self) -> usize;
    fn album_id(&self) -> Option<String>; // 新增方法
}

impl GalleryInfo for EhGallery {
    fn url(&self) -> EhGalleryUrl {
        self.url.clone()
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn title_jp(&self) -> String {
        self.title_jp.clone().unwrap_or_else(|| self.title.clone())
    }

    fn tags(&self) -> &IndexMap<String, Vec<String>> {
        &self.tags
    }

    fn pages(&self) -> usize {
        self.pages.len()
    }

    fn cover(&self) -> usize {
        self.cover
    }

    fn album_id(&self) -> Option<String> { // 获取 album_id
        self.album_id.clone()
    }
}

impl GalleryInfo for GalleryEntity {
    fn url(&self) -> EhGalleryUrl {
        format!("https://exhentai.org/g/{}/{}", self.id, self.token).parse().unwrap()
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn title_jp(&self) -> String {
        self.title_jp.clone().unwrap_or_else(|| self.title.clone())
    }

    fn tags(&self) -> &IndexMap<String, Vec<String>> {
        &self.tags.0
    }

    fn pages(&self) -> usize {
        self.pages as usize
    }

    fn cover(&self) -> usize {
        0
    }

    fn album_id(&self) -> Option<String> { // 获取 album_id
        self.album_id.clone()
    }
}

impl GalleryEntity {
    // 添加方法 update_album_id
    pub async fn update_album_id(id: i32, album_id: String) -> Result<()> {
        sqlx::query("UPDATE gallery SET album_id = $1 WHERE id = $2")
            .bind(album_id)
            .bind(id)
            .execute(&crate::DATABASE_POOL)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gallery_url() {
        let s = "https://exhentai.org/g/2423705/3962191348/";
        let url = s.parse::<EhGalleryUrl>().unwrap();
        assert_eq!(url.id, 2423705);
        assert_eq!(url.token, "3962191348");
        assert_eq!(url.url(), s);
    }

    #[test]
    fn parse_page_url() {
        let s = "https://exhentai.org/s/03af734602/1932743-1";
        let url = s.parse::<EhPageUrl>().unwrap();
        assert_eq!(url.hash, "03af734602");
        assert_eq!(url.gallery_id, 1932743);
        assert_eq!(url.page, 1);
        assert_eq!(url.url(), s);
    }

    #[test]
    fn test_album_id() {
        let gallery = EhGallery {
            url: "https://exhentai.org/g/12345/abcde".parse().unwrap(),
            title: "Test Gallery".to_string(),
            title_jp: Some("テストギャラリー".to_string()),
            tags: IndexMap::new(),
            favorite: 100,
            parent: None,
            pages: vec![],
            posted: Utc::now().naive_utc(),
            cover: 0,
            album_id: Some("album123".to_string()),  // 设置 album_id
        };

        assert_eq!(gallery.album_id(), Some("album123".to_string()));
    }
}
