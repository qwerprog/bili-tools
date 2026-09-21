use crate::error::Result;
use std::io::Write;

pub fn generate_and_save_qrcode(url: &str, filename: &std::path::Path) -> Result<()> {
    use image::Luma;
    use qrcode::QrCode;

    let code = QrCode::new(url.as_bytes())?;

    let image = code
        .render::<Luma<u8>>()
        .quiet_zone(true)
        .min_dimensions(200, 200)
        .build();

    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    crate::utils::paths::write_private(filename, &png.into_inner())?;

    Ok(())
}

pub fn print_qrcode_in_terminal(url: &str) -> Result<()> {
    use qrcode::render::unicode;
    use qrcode::{EcLevel, QrCode};

    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L)?;

    let string = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .quiet_zone(true)
        .build();

    println!("{}", string);
    std::io::stdout().flush()?;
    Ok(())
}
