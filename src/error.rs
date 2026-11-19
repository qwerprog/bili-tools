use thiserror::Error;

#[derive(Error, Debug)]
pub enum BiliLiveError {
    #[error("网络请求失败: {0}")]
    Network(#[from] minreq::Error),

    #[error("Web 登录请求失败: {0}")]
    WebNetwork(#[from] ureq::Error),

    #[error("JSON解析失败: {0}")]
    Json(#[from] serde_json::Error),

    #[error("文件操作失败: {0}")]
    Io(#[from] std::io::Error),

    #[error("数字超过范围: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("二维码生成失败: {0}")]
    QrCode(#[from] qrcode::types::QrError),

    #[error("图像处理失败: {0}")]
    Image(#[from] image::ImageError),

    #[error("API返回错误: {0}")]
    Api(String),

    #[error("用户输入错误: {0}")]
    Input(String),

    #[error("数据解析失败: {0}")]
    Parse(String),

    #[error("认证错误: {0}")]
    Auth(String),

    #[error("外部工具错误: {0}")]
    ExternalTool(String),

    #[error("操作已取消 (Ctrl+C)")]
    Interrupted,
}

impl BiliLiveError {
    pub fn error_kind(&self) -> &'static str {
        match self {
            Self::Network(_) => "Network",
            Self::WebNetwork(_) => "WebNetwork",
            Self::Json(_) => "Json",
            Self::Io(_) => "Io",
            Self::ParseInt(_) => "ParseInt",
            Self::QrCode(_) => "QrCode",
            Self::Image(_) => "Image",
            Self::Api(_) => "Api",
            Self::Input(_) => "Input",
            Self::Parse(_) => "Parse",
            Self::Auth(_) => "Auth",
            Self::ExternalTool(_) => "ExternalTool",
            Self::Interrupted => "Interrupted",
        }
    }
}

pub type Result<T> = std::result::Result<T, BiliLiveError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_returns_pascal_case_for_all_variants() {
        let json_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let parse_int_err = "abc".parse::<i64>().unwrap_err();
        let qr_err = match qrcode::QrCode::new(vec![0u8; 10000]) {
            Err(e) => e,
            Ok(_) => panic!("expected QrError"),
        };
        let img_err = image::ImageError::Limits(image::error::LimitError::from_kind(
            image::error::LimitErrorKind::DimensionError,
        ));
        #[allow(invalid_from_utf8)]
        let utf8_err = std::str::from_utf8(&[0xff]).unwrap_err();
        let minreq_err = minreq::Error::InvalidUtf8InBody(utf8_err);
        let ureq_err = ureq::Error::Timeout(ureq::Timeout::Global);

        assert_eq!(BiliLiveError::Network(minreq_err).error_kind(), "Network");
        assert_eq!(BiliLiveError::WebNetwork(ureq_err).error_kind(), "WebNetwork");
        assert_eq!(BiliLiveError::Json(json_err).error_kind(), "Json");
        assert_eq!(BiliLiveError::Io(io_err).error_kind(), "Io");
        assert_eq!(BiliLiveError::ParseInt(parse_int_err).error_kind(), "ParseInt");
        assert_eq!(BiliLiveError::QrCode(qr_err).error_kind(), "QrCode");
        assert_eq!(BiliLiveError::Image(img_err).error_kind(), "Image");
        assert_eq!(BiliLiveError::Api("api".into()).error_kind(), "Api");
        assert_eq!(BiliLiveError::Input("input".into()).error_kind(), "Input");
        assert_eq!(BiliLiveError::Parse("parse".into()).error_kind(), "Parse");
        assert_eq!(BiliLiveError::Auth("auth".into()).error_kind(), "Auth");
        assert_eq!(
            BiliLiveError::ExternalTool("ffmpeg".into()).error_kind(),
            "ExternalTool"
        );
    }
}
