// Copyright 2026 the Imaging Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Defensive validation helpers for `imaging`.
//!
//! This module provides [`ValidatingSink`], a wrapper around a [`Sink`] that checks streamed
//! commands for common validity issues before forwarding them to the wrapped sink.

use crate::{
    BlurredRoundedRect, Clip, Composite, Draw, Filter, Geometry, GlyphRun, Group, Paint, Sink,
    StrokeStyle,
};
use kurbo::{Affine, BezPath, Rect, RoundedRect};

/// Decision returned by a [`ValidatingSink`] violation hook.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ValidationDecision {
    /// Continue forwarding commands to the wrapped sink.
    Continue,
    /// Abort: stop forwarding commands and record the first validation error.
    Abort,
}

/// A validation error reported by [`ValidatingSink`].
#[derive(Clone, Debug, PartialEq)]
pub enum ValidationError {
    /// A value that must be finite was NaN or infinite.
    NonFinite {
        /// Short label describing what was invalid.
        what: &'static str,
    },
    /// A rectangle had invalid bounds.
    InvalidRect {
        /// Short label describing what was invalid.
        what: &'static str,
    },
    /// A rounded rectangle had invalid radii.
    InvalidRoundedRect {
        /// Short label describing what was invalid.
        what: &'static str,
    },
    /// A composite alpha was not finite or outside `0..=1`.
    InvalidAlpha,
    /// A stroke style had invalid parameters.
    InvalidStroke,
    /// A filter had invalid parameters.
    InvalidFilter,
    /// A paint payload had invalid parameters.
    InvalidPaint {
        /// Short label describing what was invalid.
        what: &'static str,
    },
    /// A glyph run had invalid parameters.
    InvalidGlyphRun {
        /// Short label describing what was invalid.
        what: &'static str,
    },
    /// A blurred rounded rect had invalid parameters.
    InvalidBlurredRoundedRect {
        /// Short label describing what was invalid.
        what: &'static str,
    },
    /// A stack pop occurred without a corresponding push.
    StackUnderflow {
        /// Which stack underflowed.
        what: &'static str,
    },
    /// The command stream ended with open clips.
    UnclosedClips {
        /// Remaining clip depth.
        depth: u32,
    },
    /// The command stream ended with open groups.
    UnclosedGroups {
        /// Remaining group depth.
        depth: u32,
    },
}

/// Default violation hook for [`ValidatingSink`].
#[inline]
pub fn default_validation_hook(_: &ValidationError) -> ValidationDecision {
    ValidationDecision::Abort
}

/// A wrapper around a [`Sink`] that validates inputs before forwarding them.
///
/// This is intended as a defensive layer for streaming command sinks and backends.
#[derive(Debug)]
pub struct ValidatingSink<S, H = fn(&ValidationError) -> ValidationDecision> {
    inner: S,
    hook: H,
    first_error: Option<ValidationError>,
    aborted: bool,
    clip_depth: u32,
    group_depth: u32,
}

impl<S> ValidatingSink<S> {
    /// Wrap a sink using the [`default_validation_hook`] (abort on first error).
    #[inline]
    pub fn new(inner: S) -> Self {
        Self::with_hook(inner, default_validation_hook)
    }
}

impl<S, H> ValidatingSink<S, H>
where
    H: FnMut(&ValidationError) -> ValidationDecision,
{
    /// Wrap a sink with a custom validation hook.
    #[inline]
    pub fn with_hook(inner: S, hook: H) -> Self {
        Self {
            inner,
            hook,
            first_error: None,
            aborted: false,
            clip_depth: 0,
            group_depth: 0,
        }
    }

    /// Borrow the wrapped sink.
    #[inline]
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Mutably borrow the wrapped sink.
    #[inline]
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Return the first validation error, if any.
    #[inline]
    pub fn first_error(&self) -> Option<&ValidationError> {
        self.first_error.as_ref()
    }

    /// Unwrap the sink, returning the inner sink and the first validation error (if any).
    #[inline]
    pub fn into_inner(self) -> (S, Option<ValidationError>) {
        (self.inner, self.first_error)
    }

    /// Validate that streaming push/pop stacks are balanced at end-of-stream.
    pub fn finish(&mut self) -> Result<(), ValidationError> {
        if self.clip_depth != 0 {
            let err = ValidationError::UnclosedClips {
                depth: self.clip_depth,
            };
            self.note_error(err.clone());
            return Err(err);
        }
        if self.group_depth != 0 {
            let err = ValidationError::UnclosedGroups {
                depth: self.group_depth,
            };
            self.note_error(err.clone());
            return Err(err);
        }
        Ok(())
    }

    fn note_error(&mut self, err: ValidationError) {
        if self.first_error.is_none() {
            self.first_error = Some(err);
        }
    }

    fn violate(&mut self, err: ValidationError) -> bool {
        self.note_error(err.clone());
        match (self.hook)(&err) {
            ValidationDecision::Continue => false,
            ValidationDecision::Abort => {
                self.aborted = true;
                true
            }
        }
    }

    fn validate_affine(&mut self, what: &'static str, xf: &Affine) -> bool {
        if xf.is_finite() {
            true
        } else {
            !self.violate(ValidationError::NonFinite { what })
        }
    }

    fn validate_rect(&mut self, what: &'static str, rect: &Rect) -> bool {
        if !rect.is_finite() {
            return !self.violate(ValidationError::NonFinite { what });
        }
        if rect.x0 <= rect.x1 && rect.y0 <= rect.y1 {
            true
        } else {
            !self.violate(ValidationError::InvalidRect { what })
        }
    }

    fn validate_rounded_rect(&mut self, what: &'static str, rounded_rect: &RoundedRect) -> bool {
        if !rounded_rect.is_finite() {
            return !self.violate(ValidationError::NonFinite { what });
        }
        let radii = rounded_rect.radii();
        if radii.top_left >= 0.0
            && radii.top_right >= 0.0
            && radii.bottom_right >= 0.0
            && radii.bottom_left >= 0.0
        {
            true
        } else {
            !self.violate(ValidationError::InvalidRoundedRect { what })
        }
    }

    fn validate_path(&mut self, what: &'static str, path: &BezPath) -> bool {
        if path.is_finite() {
            true
        } else {
            !self.violate(ValidationError::NonFinite { what })
        }
    }

    fn validate_geometry(&mut self, geometry: &Geometry) -> bool {
        match geometry {
            Geometry::Rect(rect) => self.validate_rect("Geometry::Rect", rect),
            Geometry::RoundedRect(rounded_rect) => {
                self.validate_rounded_rect("Geometry::RoundedRect", rounded_rect)
            }
            Geometry::Path(path) => self.validate_path("Geometry::Path", path),
        }
    }

    fn validate_stroke(&mut self, stroke: &StrokeStyle) -> bool {
        let ok = stroke.width.is_finite()
            && stroke.width >= 0.0
            && stroke.miter_limit.is_finite()
            && stroke.dash_offset.is_finite()
            && stroke
                .dash_pattern
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0);
        if ok {
            true
        } else {
            !self.violate(ValidationError::InvalidStroke)
        }
    }

    fn validate_composite(&mut self, composite: &Composite) -> bool {
        let alpha = composite.alpha;
        if alpha.is_finite() && (0.0..=1.0).contains(&alpha) {
            true
        } else {
            !self.violate(ValidationError::InvalidAlpha)
        }
    }

    fn validate_filter(&mut self, filter: &Filter) -> bool {
        let ok = match *filter {
            Filter::Flood { .. } => true,
            Filter::Blur {
                std_deviation_x,
                std_deviation_y,
            } => {
                std_deviation_x.is_finite()
                    && std_deviation_y.is_finite()
                    && std_deviation_x >= 0.0
                    && std_deviation_y >= 0.0
            }
            Filter::DropShadow {
                dx,
                dy,
                std_deviation_x,
                std_deviation_y,
                ..
            } => {
                dx.is_finite()
                    && dy.is_finite()
                    && std_deviation_x.is_finite()
                    && std_deviation_y.is_finite()
                    && std_deviation_x >= 0.0
                    && std_deviation_y >= 0.0
            }
            Filter::Offset { dx, dy } => dx.is_finite() && dy.is_finite(),
        };
        if ok {
            true
        } else {
            !self.violate(ValidationError::InvalidFilter)
        }
    }

    fn validate_paint(&mut self, paint: &Paint) -> bool {
        match paint {
            Paint::Solid(_) => true,
            Paint::Gradient(gradient) => self.validate_gradient(gradient),
            Paint::Image(image_brush) => self.validate_image_brush(image_brush),
        }
    }

    fn validate_gradient(&mut self, gradient: &peniko::Gradient) -> bool {
        if gradient.stops.is_empty() {
            return !self.violate(ValidationError::InvalidPaint {
                what: "Paint::Gradient::stops",
            });
        }

        if let Some(stop) = gradient.stops.iter().find(|stop| !stop.offset.is_finite()) {
            let _ = stop;
            return !self.violate(ValidationError::InvalidPaint {
                what: "Paint::Gradient::stop_offset",
            });
        }

        if !gradient
            .stops
            .windows(2)
            .all(|pair| pair[0].offset <= pair[1].offset)
        {
            return !self.violate(ValidationError::InvalidPaint {
                what: "Paint::Gradient::stop_order",
            });
        }

        if !gradient
            .stops
            .iter()
            .all(|stop| (0.0..=1.0).contains(&stop.offset))
        {
            return !self.violate(ValidationError::InvalidPaint {
                what: "Paint::Gradient::stop_range",
            });
        }

        let kind_ok = match &gradient.kind {
            peniko::GradientKind::Linear(line) => {
                line.start.x.is_finite()
                    && line.start.y.is_finite()
                    && line.end.x.is_finite()
                    && line.end.y.is_finite()
            }
            peniko::GradientKind::Radial(radial) => {
                radial.start_center.x.is_finite()
                    && radial.start_center.y.is_finite()
                    && radial.end_center.x.is_finite()
                    && radial.end_center.y.is_finite()
                    && radial.start_radius.is_finite()
                    && radial.start_radius >= 0.0
                    && radial.end_radius.is_finite()
                    && radial.end_radius >= 0.0
            }
            peniko::GradientKind::Sweep(sweep) => {
                sweep.center.x.is_finite()
                    && sweep.center.y.is_finite()
                    && sweep.start_angle.is_finite()
                    && sweep.end_angle.is_finite()
            }
        };
        if kind_ok {
            true
        } else {
            !self.violate(ValidationError::InvalidPaint {
                what: "Paint::Gradient::kind",
            })
        }
    }

    fn validate_image_brush(&mut self, image_brush: &peniko::ImageBrush) -> bool {
        if !(image_brush.sampler.alpha.is_finite() && image_brush.sampler.alpha >= 0.0) {
            return !self.violate(ValidationError::InvalidPaint {
                what: "Paint::Image::alpha",
            });
        }

        let image = &image_brush.image;
        if image
            .format
            .size_in_bytes(image.width, image.height)
            .is_none_or(|expected| expected != image.data.len())
        {
            return !self.violate(ValidationError::InvalidPaint {
                what: "Paint::Image::data_len",
            });
        }

        true
    }

    fn validate_glyph_run(&mut self, glyph_run: &GlyphRun) -> bool {
        let glyphs_ok = glyph_run
            .glyphs
            .iter()
            .all(|glyph| glyph.x.is_finite() && glyph.y.is_finite());
        let font_size_ok = glyph_run.font_size.is_finite() && glyph_run.font_size >= 0.0;
        let ok = self.validate_affine("GlyphRun::transform", &glyph_run.transform)
            && glyph_run.glyph_transform.as_ref().is_none_or(|transform| {
                self.validate_affine("GlyphRun::glyph_transform", transform)
            })
            && font_size_ok
            && glyphs_ok
            && self.validate_paint(&glyph_run.paint)
            && match glyph_run.style {
                peniko::Style::Fill(_) => true,
                peniko::Style::Stroke(ref stroke) => self.validate_stroke(stroke),
            }
            && self.validate_composite(&glyph_run.composite);
        if ok {
            true
        } else if !font_size_ok {
            !self.violate(ValidationError::InvalidGlyphRun {
                what: "GlyphRun::font_size",
            })
        } else if !glyphs_ok {
            !self.violate(ValidationError::InvalidGlyphRun {
                what: "GlyphRun::glyphs",
            })
        } else {
            false
        }
    }

    fn validate_blurred_rounded_rect(&mut self, draw: &BlurredRoundedRect) -> bool {
        let radius_ok = draw.radius.is_finite() && draw.radius >= 0.0;
        let std_dev_ok = draw.std_dev.is_finite() && draw.std_dev >= 0.0;
        let ok = self.validate_affine("BlurredRoundedRect::transform", &draw.transform)
            && self.validate_rect("BlurredRoundedRect::rect", &draw.rect)
            && self.validate_composite(&draw.composite)
            && radius_ok
            && std_dev_ok;
        if ok {
            true
        } else if !radius_ok {
            !self.violate(ValidationError::InvalidBlurredRoundedRect {
                what: "BlurredRoundedRect::radius",
            })
        } else if !std_dev_ok {
            !self.violate(ValidationError::InvalidBlurredRoundedRect {
                what: "BlurredRoundedRect::std_dev",
            })
        } else {
            false
        }
    }

    fn validate_clip(&mut self, clip: &Clip, transform_name: &'static str) -> bool {
        match clip {
            Clip::Fill {
                transform, shape, ..
            } => self.validate_affine(transform_name, transform) && self.validate_geometry(shape),
            Clip::Stroke {
                transform,
                shape,
                stroke,
            } => {
                self.validate_affine(transform_name, transform)
                    && self.validate_geometry(shape)
                    && self.validate_stroke(stroke)
            }
        }
    }
}

impl<S, H> Sink for ValidatingSink<S, H>
where
    S: Sink,
    H: FnMut(&ValidationError) -> ValidationDecision,
{
    fn push_clip(&mut self, clip: Clip) {
        if self.aborted {
            return;
        }

        if !self.validate_clip(&clip, "Clip::transform") {
            return;
        }

        self.clip_depth += 1;
        self.inner.push_clip(clip);
    }

    fn pop_clip(&mut self) {
        if self.aborted {
            return;
        }
        if self.clip_depth == 0 {
            let _ = self.violate(ValidationError::StackUnderflow { what: "clip" });
            return;
        }
        self.clip_depth -= 1;
        self.inner.pop_clip();
    }

    fn push_group(&mut self, group: Group) {
        if self.aborted {
            return;
        }

        let mut ok = self.validate_composite(&group.composite);
        if let Some(clip) = group.clip.as_ref() {
            ok &= self.validate_clip(clip, "Group::clip::transform");
        }
        for filter in &group.filters {
            ok &= self.validate_filter(filter);
        }
        if !ok {
            return;
        }

        self.group_depth += 1;
        self.inner.push_group(group);
    }

    fn pop_group(&mut self) {
        if self.aborted {
            return;
        }
        if self.group_depth == 0 {
            let _ = self.violate(ValidationError::StackUnderflow { what: "group" });
            return;
        }
        self.group_depth -= 1;
        self.inner.pop_group();
    }

    fn draw(&mut self, draw: Draw) {
        if self.aborted {
            return;
        }

        let ok = match &draw {
            Draw::Fill {
                transform,
                paint,
                paint_transform,
                shape,
                composite,
                ..
            } => {
                self.validate_affine("Draw::Fill::transform", transform)
                    && self.validate_paint(paint)
                    && paint_transform
                        .as_ref()
                        .is_none_or(|xf| self.validate_affine("Draw::Fill::paint_transform", xf))
                    && self.validate_geometry(shape)
                    && self.validate_composite(composite)
            }
            Draw::Stroke {
                transform,
                paint,
                paint_transform,
                stroke,
                shape,
                composite,
                ..
            } => {
                self.validate_affine("Draw::Stroke::transform", transform)
                    && self.validate_paint(paint)
                    && paint_transform
                        .as_ref()
                        .is_none_or(|xf| self.validate_affine("Draw::Stroke::paint_transform", xf))
                    && self.validate_stroke(stroke)
                    && self.validate_geometry(shape)
                    && self.validate_composite(composite)
            }
            Draw::GlyphRun(glyph_run) => self.validate_glyph_run(glyph_run),
            Draw::BlurredRoundedRect(blurred_rounded_rect) => {
                self.validate_blurred_rounded_rect(blurred_rounded_rect)
            }
        };
        if !ok {
            return;
        }

        self.inner.draw(draw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Composite, FillRule, Geometry, Glyph, Paint, Scene};
    use alloc::sync::Arc;
    use alloc::vec;
    use alloc::vec::Vec;
    use kurbo::Rect;
    use peniko::{
        Blob, Brush, Color, FontData, Gradient, ImageAlphaType, ImageBrush, ImageData, ImageFormat,
    };

    #[test]
    fn validating_sink_records_nan_and_aborts_by_default() {
        let inner = Scene::new();
        let mut sink = ValidatingSink::new(inner);
        sink.draw(Draw::Fill {
            transform: Affine::translate((f64::NAN, 0.0)),
            fill_rule: FillRule::NonZero,
            paint: Paint::default(),
            paint_transform: None,
            shape: Geometry::Rect(Rect::new(0.0, 0.0, 1.0, 1.0)),
            composite: Composite::default(),
        });
        assert!(matches!(
            sink.first_error(),
            Some(ValidationError::NonFinite { .. })
        ));
        assert!(sink.inner().commands().is_empty());
    }

    #[test]
    fn validating_sink_hook_can_continue() {
        let inner = Scene::new();
        let mut sink = ValidatingSink::with_hook(inner, |_err| ValidationDecision::Continue);
        sink.draw(Draw::Fill {
            transform: Affine::translate((f64::NAN, 0.0)),
            fill_rule: FillRule::NonZero,
            paint: Paint::default(),
            paint_transform: None,
            shape: Geometry::Rect(Rect::new(0.0, 0.0, 1.0, 1.0)),
            composite: Composite::default(),
        });
        assert!(sink.first_error().is_some());
        assert_eq!(sink.inner().commands().len(), 1);
    }

    #[test]
    fn finish_catches_unclosed_stacks() {
        let inner = Scene::new();
        let mut sink = ValidatingSink::new(inner);
        sink.push_clip(Clip::Fill {
            transform: Affine::IDENTITY,
            shape: Geometry::Rect(Rect::new(0.0, 0.0, 1.0, 1.0)),
            fill_rule: FillRule::NonZero,
        });
        assert_eq!(
            sink.finish(),
            Err(ValidationError::UnclosedClips { depth: 1 })
        );
        assert_eq!(
            sink.first_error(),
            Some(&ValidationError::UnclosedClips { depth: 1 })
        );
    }

    #[test]
    fn glyph_runs_validate_positions_and_font_size() {
        let inner = Scene::new();
        let mut sink = ValidatingSink::new(inner);
        sink.draw(Draw::GlyphRun(GlyphRun {
            font: FontData::new(Blob::new(Arc::new([0_u8, 1_u8, 2_u8, 3_u8])), 0),
            transform: Affine::IDENTITY,
            glyph_transform: None,
            font_size: -1.0,
            hint: false,
            normalized_coords: Vec::default(),
            style: peniko::Style::Fill(FillRule::NonZero),
            glyphs: vec![Glyph {
                id: 1,
                x: 0.0,
                y: f32::NAN,
            }],
            paint: Paint::Solid(Color::BLACK),
            composite: Composite::default(),
        }));
        assert_eq!(
            sink.first_error(),
            Some(&ValidationError::InvalidGlyphRun {
                what: "GlyphRun::font_size",
            })
        );
    }

    #[test]
    fn blurred_rounded_rect_validates_sigma() {
        let inner = Scene::new();
        let mut sink = ValidatingSink::new(inner);
        sink.draw(Draw::BlurredRoundedRect(BlurredRoundedRect {
            transform: Affine::IDENTITY,
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::BLACK,
            radius: 4.0,
            std_dev: -1.0,
            composite: Composite::default(),
        }));
        assert_eq!(
            sink.first_error(),
            Some(&ValidationError::InvalidBlurredRoundedRect {
                what: "BlurredRoundedRect::std_dev",
            })
        );
    }

    #[test]
    fn gradients_validate_stop_offsets() {
        let inner = Scene::new();
        let mut sink = ValidatingSink::new(inner);
        sink.draw(Draw::Fill {
            transform: Affine::IDENTITY,
            fill_rule: FillRule::NonZero,
            paint: Brush::Gradient(
                Gradient::new_linear((0.0, 0.0), (10.0, 0.0))
                    .with_stops([(0.5, Color::BLACK), (0.25, Color::WHITE)]),
            ),
            paint_transform: None,
            shape: Geometry::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)),
            composite: Composite::default(),
        });
        assert_eq!(
            sink.first_error(),
            Some(&ValidationError::InvalidPaint {
                what: "Paint::Gradient::stop_order",
            })
        );
    }

    #[test]
    fn image_brushes_validate_byte_length() {
        let inner = Scene::new();
        let mut sink = ValidatingSink::new(inner);
        sink.draw(Draw::Fill {
            transform: Affine::IDENTITY,
            fill_rule: FillRule::NonZero,
            paint: Brush::Image(ImageBrush::new(ImageData {
                data: Blob::from(vec![0_u8; 3]),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::Alpha,
                width: 1,
                height: 1,
            })),
            paint_transform: None,
            shape: Geometry::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)),
            composite: Composite::default(),
        });
        assert_eq!(
            sink.first_error(),
            Some(&ValidationError::InvalidPaint {
                what: "Paint::Image::data_len",
            })
        );
    }
}
