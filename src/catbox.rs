use reqwest::Client;
use reqwest::multipart::{Form, Part};
use anyhow::{Context, Result};
use std::time::Duration;

pub struct CatboxUploader {
    api_url: String,
    userhash: String,
    client: Client,
}

impl CatboxUploader {
    // 构造函数
    pub fn new(api_url: &str, userhash: &str) -> Self {
        Self {
            api_url: api_url.to_string(),
            userhash: userhash.to_string(),
            client: Client::new(),
        }
    }

    // 上传文件方法
    pub async fn upload_file(
        &self,
        file_name: &str,
        file_bytes: &[u8],
    ) -> Result<String> {
        let form = Form::new()
            .text("reqtype", "fileupload") // 请求类型
            .text("userhash", self.userhash.clone()) // 用户哈希（可以为空，用于匿名上传）
            .part("fileToUpload", Part::bytes(file_bytes.to_vec()).file_name(file_name.to_string()));

        println!("Sending request to: {}", &self.api_url);
        println!("Userhash: {}", self.userhash);

        let res = self.client
            .post(&self.api_url)
            .multipart(form)
            .header("User-Agent", "exloli-client/1.0")
            .timeout(Duration::from_secs(30))
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status(); // 提取状态码
                let text = response.text().await.context("Failed to read response body")?;

                if status.is_success() {
                println!("Response from Catbox: {}", text);
                if text.starts_with("https://files.catbox.moe/") {
                    Ok(text) // 返回完整的 URL
                } else {
                    Err(anyhow::anyhow!(format!(
                        "响应中返回的不是有效 URL，响应内容: {}",
                        text
                    )))
                }
            } else {
                Err(anyhow::anyhow!(format!(
                    "上传失败: 状态码: {}, 响应内容: {}",
                    status,
                    text
                )))
            }
        }
        Err(err) => Err(anyhow::anyhow!("Failed to send request to Catbox API: {:?}", err)),
    }


    // 创建专辑
    pub async fn create_album(
        &self,
        title: &str,
        description: &str,
        files: &[&str], // 文件链接数组，直接使用完整链接
    ) -> Result<String> {
        let file_list = files.join(" "); // 拼接文件列表，用空格分隔
        let form = Form::new()
            .text("reqtype", "createalbum")
            .text("userhash", self.userhash.clone())
            .text("title", title.to_string())
            .text("desc", description.to_string())
            .text("files", file_list);

        let res = self.client
            .post(&self.api_url)
            .multipart(form)
            .header("User-Agent", "exloli-client/1.0")
            .timeout(Duration::from_secs(30))
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.context("Failed to read response body")?;
                if status.is_success() {
                    println!("Album created: {}", text);
                    Ok(text)
                } else {
                    Err(anyhow::anyhow!(format!(
                        "创建专辑失败: 状态码: {}, 响应内容: {}",
                        status,
                        text
                    )))
                }
            }
            Err(err) => Err(anyhow::anyhow!(format!(
                "请求创建专辑时出错: {:?}",
                err
            ))),
        }
    }

    // 添加文件到专辑
    pub async fn add_to_album(&self, short: &str, files: &[&str]) -> Result<()> {
        let file_list = files.join(" "); // 直接使用完整文件链接
        let form = Form::new()
            .text("reqtype", "addtoalbum")
            .text("userhash", self.userhash.clone())
            .text("short", short.to_string())
            .text("files", file_list);

        let res = self.client
            .post(&self.api_url)
            .multipart(form)
            .header("User-Agent", "exloli-client/1.0")
            .timeout(Duration::from_secs(30))
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.context("Failed to read response body")?;
                if status.is_success() {
                    println!("Files added to album: {}", text);
                    Ok(())
                } else {
                    Err(anyhow::anyhow!(format!(
                        "添加文件到专辑失败: 状态码: {}, 响应内容: {}",
                        status,
                        text
                    )))
                }
            }
            Err(err) => Err(anyhow::anyhow!(format!(
                "请求添加文件到专辑时出错: {:?}",
                err
            ))),
        }
    }
}
