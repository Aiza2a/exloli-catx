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
        // 构造 multipart 请求体
        let form = Form::new()
            .text("reqtype", "fileupload") // 请求类型
            .text("userhash", self.userhash.clone()) // 用户哈希（可以为空，用于匿名上传）
            .part("fileToUpload", Part::bytes(file_bytes.to_vec()).file_name(file_name.to_string())); // 上传文件

        println!("Sending request to: {}", &self.api_url);
        println!("Userhash: {}", self.userhash);

        // 发送 POST 请求
        let res = self.client
            .post(&self.api_url) // Catbox API URL
            .multipart(form) // 添加 multipart 表单
            .header("User-Agent", "exloli-client/1.0") // 添加 User-Agent 头，防止被屏蔽
            .timeout(Duration::from_secs(30)) // 设置请求超时时间
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status(); // 提取状态码
                let text = response.text().await.context("Failed to read response body")?; // 获取响应文本

                if status.is_success() {
                    println!("Response from Catbox: {}", text);
                    if text.starts_with("https://files.catbox.moe/") {
                        Ok(text) // 返回上传后的文件 URL
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
            Err(err) => {
                eprintln!("请求失败: {:?}", err);
                Err(anyhow::anyhow!("Failed to send request to Catbox API"))
            }
        }
    }

    // 创建专辑
    pub async fn create_album(
        &self,
        title: &str,
        description: &str,
        files: &[&str],
    ) -> Result<String> {
        let file_list = files.join(" ");
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
                let status = response.status(); // 提取状态码
                let text = response.text().await.context("Failed to read response body")?;

                if status.is_success() {
                    println!("Album created: {}", text);
                    Ok(text) // 返回专辑 ID
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

    // 将文件添加到专辑
    pub async fn add_to_album(&self, short: &str, files: &[&str]) -> Result<()> {
        let file_list = files.join(" ");
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
                let status = response.status(); // 提取状态码
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
