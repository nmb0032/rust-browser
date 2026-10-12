use std::{error::Error, io, io::Cursor};

use crate::display::ColorProfile;

pub(crate) const MAX_PROFILE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageColorSource {
    UntaggedSrgb,
    Srgb,
    Icc,
    PngCicp,
    PngGamma,
}

pub(crate) fn icc(bytes: &[u8]) -> Result<moxcms::ColorProfile, Box<dyn Error>> {
    let profile = moxcms::ColorProfile::new_from_slice_with_options(
        bytes,
        moxcms::ParsingOptions {
            max_profile_size: MAX_PROFILE_BYTES,
            max_allowed_clut_size: MAX_PROFILE_BYTES,
            max_allowed_trc_size: 4096,
        },
    )?;
    if profile.color_space != moxcms::DataColorSpace::Rgb {
        return Err(io::Error::other("only RGB image ICC profiles are supported").into());
    }
    Ok(profile)
}

pub(crate) fn png_profile(
    bytes: &[u8],
) -> Result<(ImageColorSource, Option<moxcms::ColorProfile>), Box<dyn Error>> {
    let reader = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: MAX_PROFILE_BYTES,
        },
    )
    .read_info()?;
    let info = reader.info();
    // PNG 3 precedence: cICP, iCCP, sRGB, cHRM/gAMA.
    if let Some(cicp) = info.coding_independent_code_points {
        if cicp.transfer_function != 13
            || cicp.matrix_coefficients != 0
            || !cicp.is_video_full_range_image
        {
            return Err(io::Error::other(
                "unsupported PNG cICP: SDR full-range RGB/sRGB transfer required",
            )
            .into());
        }
        let profile = match cicp.color_primaries {
            1 => ColorProfile::Srgb,
            12 => ColorProfile::DisplayP3,
            _ => return Err(io::Error::other("unsupported PNG cICP primaries").into()),
        };
        return Ok((ImageColorSource::PngCicp, Some(profile.icc())));
    }
    if let Some(bytes) = &info.icc_profile {
        return Ok((ImageColorSource::Icc, Some(icc(bytes)?)));
    }
    if info.srgb.is_some() {
        return Ok((ImageColorSource::Srgb, None));
    }
    let mut profile = ColorProfile::Srgb.icc();
    if let Some(chrm) = info.chrm_chunk {
        let srgb = png::SourceChromaticities::new(
            (0.3127, 0.3290),
            (0.64, 0.33),
            (0.30, 0.60),
            (0.15, 0.06),
        );
        let p3 = png::SourceChromaticities::new(
            (0.3127, 0.3290),
            (0.68, 0.32),
            (0.265, 0.69),
            (0.15, 0.06),
        );
        if chrm == p3 {
            profile = ColorProfile::DisplayP3.icc();
        } else if chrm != srgb {
            return Err(io::Error::other(
                "unsupported PNG cHRM primaries; supply an RGB ICC profile",
            )
            .into());
        }
    }
    if let Some(gamma) = info.gama_chunk {
        let gamma = gamma.into_value();
        if !(0.1..=10.0).contains(&gamma) {
            return Err(io::Error::other("PNG gamma must be finite 0.1..10").into());
        }
        let curve = moxcms::ToneReprCurve::Parametric(vec![1.0 / gamma]);
        profile.red_trc = Some(curve.clone());
        profile.green_trc = Some(curve.clone());
        profile.blue_trc = Some(curve);
        profile.cicp = None;
    }
    if info.chrm_chunk.is_some() || info.gama_chunk.is_some() {
        Ok((ImageColorSource::PngGamma, Some(profile)))
    } else {
        Ok((ImageColorSource::UntaggedSrgb, None))
    }
}

pub(crate) fn convert(
    rgba: &[u8],
    source: &moxcms::ColorProfile,
    target: ColorProfile,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let transform = source.create_transform_8bit(
        moxcms::Layout::Rgba,
        &target.icc(),
        moxcms::Layout::Rgba,
        moxcms::TransformOptions {
            rendering_intent: moxcms::RenderingIntent::RelativeColorimetric,
            prefer_fixed_point: false,
            ..Default::default()
        },
    )?;
    let mut output = vec![0; rgba.len()];
    transform.transform(rgba, &mut output)?;
    // Color transforms never own alpha, including RGB LUT profile transforms.
    for (source, target) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(output.as_chunks_mut::<4>().0.iter_mut())
    {
        target[3] = source[3];
    }
    Ok(output)
}
