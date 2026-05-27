use crate::error::Result;
use std::io::Write;

pub fn generate_and_save_qrcode(url: &str, filename: &str) -> Result<()> {
    use image::Luma;
    use qrcode::QrCode;
    use std::path::Path;

    let code = QrCode::new(url.as_bytes())?;

    let image = code
        .render::<Luma<u8>>()
        .quiet_zone(false)
        .min_dimensions(200, 200)
        .build();

    let path = Path::new(filename);
    image.save(path)?;

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
