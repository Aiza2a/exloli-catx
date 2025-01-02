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
            .text("reqtype", "fileupload")  // 请求类型
            .text("userhash", self.userhash.clone())  // 用户哈希（可以为空，用于匿名上传）
            .part("fileToUpload", Part::bytes(file_bytes.to_vec()).file_name(file_name.to_string()));  // 上传文件

        // 打印请求的 URL 和请求体内容（供调试）
        println!("Sending request to: {}", &self.api_url);
        println!("Userhash: {}", self.userhash);

        // 发送 POST 请求
        let res = self.client
            .post(&self.api_url)  // Catbox API URL
            .multipart(form)      // 添加 multipart 表单
            .header("User-Agent", "exloli-client/1.0")  // 添加 User-Agent 头，防止被屏蔽
            .timeout(Duration::from_secs(30))  // 设置请求超时时间
            .send()
            .await;

        match res {
            Ok(response) => {
                // 获取响应的状态码
                let status = response.status();
                // 获取响应文本内容
                let text = response.text().await.context("Failed to read response body")?;

                // 检查响应状态码
                if !status.is_success() {
                    let error_msg = format!(
                        "上传失败: 状态码: {}, 响应内容: {}",
                        status,
                        text
                    );
                    return Err(anyhow::anyhow!(error_msg));  // 返回错误信息
                }

                // 打印响应内容（供调试）
                println!("Response from Catbox: {}", text);

                // Catbox 返回的文本格式是 URL
                if text.starts_with("https://files.catbox.moe/") {
                    Ok(text)  // 返回上传后的文件 URL
                } else {
                    let error_msg = format!(
                        "响应中返回的不是有效 URL，响应内容: {}",
                        text
                    );
                    Err(anyhow::anyhow!(error_msg))  // 返回错误
                }
            }
            Err(err) => {
                // 捕获并打印具体的错误类型
                eprintln!("请求失败: {:?}", err);
                Err(anyhow::anyhow!("Failed to send request to Catbox API"))
            }
        }
    }
}

impl CatboxUploader {
    // Create an album
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
                let text = response.text().await.context("Failed to read response body")?;
                if response.status().is_success() {
                    println!("Album created: {}", text);
                    Ok(text) // Return the album ID
                } else {
                    Err(anyhow::anyhow!(format!("Failed to create album: {}", text)))
                }
            }
            Err(err) => Err(anyhow::anyhow!("Error in creating album: {:?}", err)),
        }
    }

    // Add files to an album
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

        if let Ok(response) = res {
            let text = response.text().await.context("Failed to read response body")?;
            if response.status().is_success() {
                println!("Files added to album: {}", text);
                Ok(())
            } else {
                Err(anyhow::anyhow!(format!("Failed to add files to album: {}", text)))
            }
        } else {
            Err(anyhow::anyhow!("Error adding files to album"))
        }
    }
}
