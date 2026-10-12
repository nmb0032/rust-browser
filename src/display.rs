//! Explicit output encoding and a single CSS/device coordinate contract.
use crate::css::values::{ColorSpace, ResolvedColor};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterOptions {
    pub scale: f64,
    pub scroll: f32,
    pub profile: ColorProfile,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorProfile {
    #[default]
    Srgb,
    DisplayP3,
}

impl ColorProfile {
    pub fn space(self) -> ColorSpace {
        match self {
            Self::Srgb => ColorSpace::Srgb,
            Self::DisplayP3 => ColorSpace::DisplayP3,
        }
    }

    pub fn color(self, color: ResolvedColor) -> (u32, u32) {
        color.to_rgb8(self.space())
    }

    pub fn rgb(self, rgb: u32) -> u32 {
        self.color(ResolvedColor::from_rgb(rgb)).0
    }

    pub fn icc(self) -> moxcms::ColorProfile {
        let mut profile = match self {
            Self::Srgb => moxcms::ColorProfile::new_srgb(),
            Self::DisplayP3 => moxcms::ColorProfile::new_display_p3(),
        };
        // moxcms 0.8's built-in P3 colorants/TRCs are correct, but its optional
        // cICP tag says cinema P3/limited YCbCr. Output is D65, full-range RGB.
        if let Some(cicp) = &mut profile.cicp {
            cicp.color_primaries = match self {
                Self::Srgb => moxcms::CicpColorPrimaries::Bt709,
                Self::DisplayP3 => moxcms::CicpColorPrimaries::Smpte432,
            };
            cicp.matrix_coefficients = moxcms::MatrixCoefficients::Identity;
            cicp.full_range = true;
        }
        profile.rendering_intent = moxcms::RenderingIntent::RelativeColorimetric;
        profile
    }

    pub(crate) fn encoded_icc(self) -> Result<Vec<u8>, moxcms::CmsError> {
        let mut bytes = self.icc().encode()?;
        // The CMS writer uses wall time even for immutable standard profiles.
        // Pin the ICC header timestamp (profile ID is zero) for byte-stable PNGs.
        for (pair, value) in bytes[24..36]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip([2026u16, 1, 1, 0, 0, 0])
        {
            *pair = value.to_be_bytes();
        }
        Ok(bytes)
    }

    /// softbuffer has no tagged surface/profile API on any supported backend.
    /// Do not feed P3-encoded bytes into its untagged 0x00RRGGBB transport.
    pub fn validate_native(self) -> Result<(), &'static str> {
        if self == Self::Srgb {
            Ok(())
        } else {
            Err(
                "native softbuffer cannot tag/present Display-P3; use --screenshot --color-profile display-p3 for an ICC-tagged PNG",
            )
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewScale {
    pub dpr: f64,
    pub zoom: f64,
}

impl Default for ViewScale {
    fn default() -> Self {
        Self {
            dpr: 1.0,
            zoom: 1.0,
        }
    }
}

impl ViewScale {
    pub fn new(dpr: f64, zoom: f64) -> Result<Self, &'static str> {
        if !dpr.is_finite()
            || !zoom.is_finite()
            || !(0.125..=8.0).contains(&dpr)
            || !(0.25..=4.0).contains(&zoom)
            || !(0.125..=8.0).contains(&(dpr * zoom))
        {
            return Err("DPR must be finite 0.125..8, page zoom 0.25..4, and DPR*zoom 0.125..8");
        }
        Ok(Self { dpr, zoom })
    }

    pub fn effective(self) -> f64 {
        self.dpr * self.zoom
    }

    pub fn css_viewport(self, physical_width: u32, physical_height: u32) -> (f32, f32) {
        (
            (f64::from(physical_width) / self.effective()) as f32,
            (f64::from(physical_height) / self.effective()) as f32,
        )
    }
}
