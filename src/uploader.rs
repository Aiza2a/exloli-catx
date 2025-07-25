use std::backtrace::Backtrace;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use chrono::{Datelike, Utc};
use futures::StreamExt;
use regex::Regex;
use reqwest::{Client, StatusCode};
use telegraph_rs::{html_to_node, Telegraph};
use teloxide::prelude::*;
use teloxide::types::MessageId;
//use teloxide::utils::html::{code_inline, link};
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{debug, error, info};

use crate::bot::Bot;
use crate::catbox::CatboxUploader;
use crate::config::Config;
use crate::database::{
    GalleryEntity, ImageEntity, MessageEntity, PageEntity, PollEntity, TelegraphEntity,
};
use crate::ehentai::{EhClient, EhGallery, EhGalleryUrl, GalleryInfo};
use crate::tags::EhTagTransDB;

#[derive(Debug, Clone)]
pub struct ExloliUploader {
    ehentai: EhClient,
    telegraph: Telegraph,
    bot: Bot,
    config: Config,
    trans: EhTagTransDB,
}

impl ExloliUploader {
    pub async fn new(
        config: Config,
        ehentai: EhClient,
        bot: Bot,
        trans: EhTagTransDB,
    ) -> Result<Self> {
        let telegraph = Telegraph::new(&config.telegraph.author_name)
            .author_url(&config.telegraph.author_url)
            .access_token(&config.telegraph.access_token)
            .create()
            .await?;
        Ok(Self {
            ehentai,
            config,
            telegraph,
            bot,
            trans,
        })
    }

    /// 每隔 interval 分钟检查一次
    pub async fn start(&self) {
        loop {
            info!("开始扫描 E 站 本子");
            self.check().await;
            info!("扫描完毕，等待 {:?} 后继续", self.config.interval);
            time::sleep(self.config.interval).await;
        }
    }

    /// 根据配置文件，扫描前 N 个本子，并进行上传或者更新
    #[tracing::instrument(skip(self))]
    async fn check(&self) {
        let stream = self
            .ehentai
            .search_iter(&self.config.exhentai.search_params)
            .take(self.config.exhentai.search_count);
        tokio::pin!(stream);
        while let Some(next) = stream.next().await {
            // 错误不要上抛，避免影响后续画廊
            if let Err(err) = self.try_update(&next, true).await {
                error!(
                    "check_and_update: {:?}\n{}",
                    err,
                    Backtrace::force_capture()
                );
            }
            if let Err(err) = self.try_upload(&next, true).await {
                error!(
                    "check_and_upload: {:?}\n{}",
                    err,
                    Backtrace::force_capture()
                );
            }
            time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// 检查指定画廊是否已经上传，如果没有则进行上传
    ///
    /// 为了避免绕晕自己，这次不考虑父子画廊，只要 id 不同就视为新画廊，只要是新画廊就进行上传
    #[tracing::instrument(skip(self))]
    pub async fn try_upload(&self, gallery_url_param: &EhGalleryUrl, check: bool) -> Result<()> {
        if check
            && GalleryEntity::check(gallery_url_param.id()).await?
            && MessageEntity::get_by_gallery(gallery_url_param.id())
                .await?
                .is_some()
        {
            return Ok(());
        }

        let gallery_data = self.ehentai.get_gallery(gallery_url_param).await?;
        // 上传图片、发布文章
        let catbox_album_url = self.upload_gallery_image(&gallery_data).await?;
        let article = self.publish_telegraph_article(&gallery_data).await?;
        // 发送消息
        let text = self
            .create_message_text(
                &gallery_data,
                &article.url,
                catbox_album_url.as_deref(),
            )
            .await?;
        // FIXME: 此处没有考虑到父画廊没有上传，但是父父画廊上传过的情况
        // 不过一般情况下画廊应该不会那么短时间内更新多次
        let msg = if let Some(parent) = &gallery_data.parent {
            if let Some(pmsg) = MessageEntity::get_by_gallery(parent.id()).await? {
                self.bot
                    .send_message(self.config.telegram.channel_id.clone(), text)
                    .reply_to_message_id(MessageId(pmsg.id))
                    .await?
            } else {
                self.bot
                    .send_message(self.config.telegram.channel_id.clone(), text)
                    .await?
            }
        } else {
            self.bot
                .send_message(self.config.telegram.channel_id.clone(), text)
                .await?
        };
        // 数据入库
        MessageEntity::create(msg.id.0, gallery_data.url.id()).await?;
        TelegraphEntity::create(gallery_data.url.id(), &article.url).await?;
        GalleryEntity::create(&gallery_data).await?;
        // 可选：将 catbox_album_url 保存到数据库，例如一个新的表或者扩展 TelegraphEntity
        // if let Some(album_url) = catbox_album_url {
        //     // CatboxAlbumEntity::create(gallery_data.url.id(), &album_url).await?;
        // }

        Ok(())
    }

    /// 检查指定画廊是否有更新，比如标题、标签
    #[tracing::instrument(skip(self))]
    pub async fn try_update(&self, gallery_url_param: &EhGalleryUrl, check: bool) -> Result<()> {
        let entity = match GalleryEntity::get(gallery_url_param.id()).await? {
            Some(v) => v,
            _ => return Ok(()),
        };
        let message = match MessageEntity::get_by_gallery(gallery_url_param.id()).await? {
            Some(v) => v,
            _ => return Ok(()),
        };

        // 2 天内创建的画廊，每天都尝试更新
        // 7 天内创建的画廊，每 3 天尝试更新
        // 14 天内创建的画廊，每 7 天尝试更新
        // 其余的，每 14 天尝试更新
        let now = Utc::now().date_naive();
        let seed = match now - message.publish_date {
            d if d < chrono::Duration::days(2) => 1,
            d if d < chrono::Duration::days(7) => 3,
            d if d < chrono::Duration::days(14) => 7,
            _ => 14,
        };
        if check && now.day() % seed != 0 {
            return Ok(());
        }

        // 检查 tag 和标题是否有变化
        let current_gallery_data = self.ehentai.get_gallery(gallery_url_param).await?;
        let catbox_album_url = self.upload_gallery_image(&current_gallery_data).await?;

        if current_gallery_data.tags != entity.tags.0 || current_gallery_data.title != entity.title
        {
            let telegraph = TelegraphEntity::get(current_gallery_data.url.id())
                .await?
                .unwrap();
            let text = self
                .create_message_text(
                    &current_gallery_data,
                    &telegraph.url,
                    catbox_album_url.as_deref(),
                )
                .await?;
            self.bot
                .edit_message_text(
                    self.config.telegram.channel_id.clone(),
                    MessageId(message.id),
                    text,
                )
                .await?;
        }

        GalleryEntity::create(&current_gallery_data).await?;

        Ok(())
    }

    /// 重新发布指定画廊的文章，并更新消息
    pub async fn republish(&self, gallery: &GalleryEntity, msg: &MessageEntity) -> Result<()> {
        info!("重新发布：{}", msg.id);
        let article = self.publish_telegraph_article(gallery).await?;

        let eh_gallery_url = gallery.url();
        let gallery_data_for_catbox = self.ehentai.get_gallery(&eh_gallery_url).await?;
        let catbox_album_url = self.upload_gallery_image(&gallery_data_for_catbox).await?;

        let text = self
            .create_message_text(gallery, &article.url, catbox_album_url.as_deref())
            .await?;
        self.bot
            .edit_message_text(
                self.config.telegram.channel_id.clone(),
                MessageId(msg.id),
                text,
            )
            .await?;
        TelegraphEntity::update(gallery.id, &article.url).await?;
        Ok(())
    }

    /// 检查 telegraph 文章是否正常
    pub async fn check_telegraph(&self, url: &str) -> Result<bool> {
        Ok(Client::new().head(url).send().await?.status() != StatusCode::NOT_FOUND)
    }
}

impl ExloliUploader {
    async fn upload_gallery_image(&self, gallery: &EhGallery) -> Result<Option<String>> {
        // 收集需要上传的图片
        let mut pages_to_upload = vec![];
        for page in &gallery.pages {
            match ImageEntity::get_by_hash(page.hash()).await? {
                Some(img) => {
                    PageEntity::create(page.gallery_id(), page.page(), img.id).await?;
                }
                None => pages_to_upload.push(page.clone()),
            }
        }
        info!("需要上传的图片数: {}", pages_to_upload.len());

        if pages_to_upload.is_empty() && gallery.pages.is_empty() {
             // 如果画廊本身就没图片，或者所有图片都已在数据库中，则无需创建或查找相册
            return Ok(None);
        }


        let client = self.ehentai.clone();
        let catbox = CatboxUploader::new(
            &self.config.catbox.api_url,
            &self.config.catbox.userhash,
        );

        // 上传的文件短链接列表
        let mut uploaded_file_names = vec![];

        // 上传图片
        for page in pages_to_upload {
            let rst = client.get_image_url(&page).await?;
            let mut suffix = rst.1.split('.').last().unwrap_or("jpg");
            // 检查是否为 webp 格式，若是则将后缀修改为 jpg
            if suffix == "webp" {
                suffix = "jpg";
            }
            if suffix == "gif" {
                continue; // 忽略 GIF 图片
            }

            let file_name_on_catbox = format!("{}.{}", page.hash(), suffix);
            let file_bytes = reqwest::get(&rst.1).await?.bytes().await?.to_vec();
            debug!("已下载: {}", page.page());

            // 调用 CatboxUploader 上传文件
            match catbox
                .upload_file(&file_name_on_catbox, &file_bytes)
                .await
            {
                Ok(file_url_on_catbox) => {
                    debug!("已上传: {}", page.page());
                    // 记录到数据库
                    ImageEntity::create(rst.0, page.hash(), &file_url_on_catbox).await?;
                    PageEntity::create(page.gallery_id(), page.page(), rst.0).await?;

                    // 只收集文件的短链接（文件名）
                    let file_short_name = file_url_on_catbox
                        .split('/')
                        .last()
                        .unwrap_or(""); // 获取短链接部分，如：4b71m5.webp
                    if !file_short_name.is_empty() {
                        uploaded_file_names.push(file_short_name.to_string());
                    }
                }
                Err(err) => {
                    // 通常，单个图片上传失败不应阻止整个流程，但可能需要记录
                    error!("图片 {} 上传失败: {}", page.page(), err);
                }
            }
        }

        // 如果有新上传的文件，则创建专辑
        if !uploaded_file_names.is_empty() {
            let album_title = gallery.title_jp(); // 优先使用日文标题
            let album_desc = self.config.telegraph.author_name.clone(); // 描述为作者名

            match catbox
                .create_album(
                    &album_title,
                    &album_desc,
                    &uploaded_file_names
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                )
                .await
            {
                Ok(album_url) => {
                    info!("专辑创建成功，专辑 : {}", album_url);
                    Ok(Some(album_url)) // 返回专辑 URL
                }
                Err(err) => {
                    error!("专辑创建失败: {}", err);
                    // 即使专辑创建失败，图片可能已上传，所以这里返回 Ok(None) 表示没有专辑链接
                    Ok(None)
                }
            }
        } else if !gallery.pages.is_empty() && uploaded_file_names.is_empty() {
            // 没有新上传的文件，但画廊本身有图片（意味着所有图片都已在Catbox上）
            // 这种情况下，我们不创建新专辑，因为没有新的文件名可以传递给 create_album
            // 如果需要，这里可以添加逻辑来查找之前可能为这个画廊创建的专辑，但这需要数据库支持
            // 目前简单处理为：如果没有新文件上传，就不尝试创建/返回专辑链接。
            debug!("没有新的图片需要上传到Catbox，不创建新专辑。");
            Ok(None)
        }
         else {
            // 没有文件上传（pages_to_upload 为空），并且画廊本身也没有页面 (gallery.pages 为空)
            // 或者有图片但没有一个成功上传并获得文件名
            Ok(None)
        }
    }

    // 从数据库中读取某个画廊的所有图片，生成一篇 telegraph 文章
    async fn publish_telegraph_article<T: GalleryInfo>(
        &self,
        gallery: &T,
    ) -> Result<telegraph_rs::Page> {
        let images = ImageEntity::get_by_gallery_id(gallery.url().id()).await?;

        let mut html = String::new();
        if gallery.cover() != 0 && gallery.cover() < images.len() {
            html.push_str(&format!(
                r#"<img src="{}">"#,
                images[gallery.cover()].url()
            ))
        }
        for img in images {
            html.push_str(&format!(r#"<img src="{}">"#, img.url()));
        }
        html.push_str(&format!("<p>ᴘᴀɢᴇꜱ : {}</p>", gallery.pages()));

        let node = html_to_node(&html);
        // 文章标题优先使用日文
        let title = gallery.title_jp();
        Ok(self.telegraph.create_page(&title, &node, false).await?)
    }

    /// 为画廊生成一条可供发送的 telegram 消息正文
    async fn create_message_text<T: GalleryInfo>(
        &self,
        gallery: &T,
        article_url: &str,
        catbox_album_url: Option<&str>,
    ) -> Result<String> {
        // 首先，将 tag 翻译
        let re = Regex::new("[-/· ]").unwrap();
        let tags = self.trans.trans_tags(gallery.tags());
        let mut text = String::new();
        text.push_str(&format!("<b>{}</b>\n\n", gallery.title_jp()));
        for (ns, tag) in tags {
            let tag = tag
                .iter()
                .map(|s| format!("#{}", re.replace_all(s, "_")))
                .collect::<Vec<_>>()
                .join(" ");
            text.push_str(&format!("⁣⁣⁣⁣　<code>{}</code>: <i>{}</i>\n", ns, tag))
        }
        text.push_str(&format!(
            "\n<b>〔 <a href=\"{}\">即 時 預 覽</a> 〕</b>/",
            article_url
        ));
        text.push_str(&format!(
            "<b>〔 <a href=\"{}\">来 源</a> 〕</b>", // 在这里结束，如果后面有专辑链接则会加上 /
            gallery.url().url()
        ));

        if let Some(album_url) = catbox_album_url {
            text.push_str(&format!(
                "/<b>〔 <a href=\"{}\">專 輯</a> 〕</b>",
                album_url
            ));
        }
        Ok(text)
    }
}

async fn flatten<T>(handle: JoinHandle<Result<T>>) -> Result<T> {
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err),
        Err(err) => bail!(err),
    }
}

impl ExloliUploader {
    /// 重新扫描并上传没有上传过但存在记录的画廊
    pub async fn reupload(&self, mut galleries: Vec<GalleryEntity>) -> Result<()> {
        if galleries.is_empty() {
            galleries = GalleryEntity::list_scans().await?;
        }
        for gallery in galleries.iter().rev() {
            if let Some(score) = PollEntity::get_by_gallery(gallery.id).await? {
                if score.score > 0.8 {
                    info!("尝试上传画廊：{}", gallery.url());
                    if let Err(err) = self.try_upload(&gallery.url(), true).await {
                        error!("上传失败：{}", err);
                    }
                    time::sleep(Duration::from_secs(60)).await;
                }
            }
        }
        Ok(())
    }

    /// 重新检测已上传过的画廊预览是否有效，并重新上传
    pub async fn recheck(&self, mut galleries: Vec<GalleryEntity>) -> Result<()> {
        if galleries.is_empty() {
            galleries = GalleryEntity::list_scans().await?;
        }
        for gallery in galleries.iter().rev() {
            let telegraph = TelegraphEntity::get(gallery.id)
                .await?
                .ok_or(anyhow!("找不到 telegraph"))?;
            if let Some(msg) = MessageEntity::get_by_gallery(gallery.id).await? {
                info!("检测画廊：{}", gallery.url());
                if !self.check_telegraph(&telegraph.url).await? {
                    info!("重新上传预览：{}", gallery.url());
                    if let Err(err) = self.republish(gallery, &msg).await {
                        error!("上传失败：{}", err);
                    }
                    time::sleep(Duration::from_secs(60)).await;
                }
            }
            time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }
}
