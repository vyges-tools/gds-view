//! Mask colours on multi-patterned layers.
//!
//! A multi-patterned layer is drawn as one layer split across several masks, and a layout
//! stream carries the mask as a different datatype on the same layer number. Coloured by
//! layer number alone, every mask of such a layer looks the same, which hides exactly what a
//! reviewer of a coloured layout needs to see: which shapes went to which mask, and where two
//! shapes of the same mask sit close together.
//!
//! A [`MaskMap`] names which `layer/datatype` pairs are which mask. A shape on a mapped pair
//! is drawn in its layer's colour shaded by mask (mask 1 is the plain layer colour), and the
//! legend lists each layer's masks separately. A `BOX` element's boxtype plays the datatype's
//! part. Pairs the map does not name are drawn exactly
//! as before, so a render without a map is unchanged.

/// `layer/datatype` → mask number (1-based).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaskMap(Vec<((i16, i16), u8)>);

impl MaskMap {
    /// Parse `L/D=M` entries separated by commas, e.g. `15/0=1,15/1=2`.
    ///
    /// Mask numbers start at 1. A pair named twice is refused rather than resolved by order:
    /// two masks for one datatype is a mistake in the map, not a preference.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut v: Vec<((i16, i16), u8)> = Vec::new();
        for tok in spec.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let bad = || format!("--masks entry {tok:?}: expected `layer/datatype=mask`");
            let (ld, m) = tok.split_once('=').ok_or_else(bad)?;
            let (l, d) = ld.split_once('/').ok_or_else(bad)?;
            let l: i16 = l.trim().parse().map_err(|_| bad())?;
            let d: i16 = d.trim().parse().map_err(|_| bad())?;
            let m: u8 = m.trim().parse().map_err(|_| bad())?;
            if m == 0 {
                return Err(format!("--masks entry {tok:?}: masks are numbered from 1"));
            }
            if v.iter().any(|(k, _)| *k == (l, d)) {
                return Err(format!("--masks names {l}/{d} twice"));
            }
            v.push(((l, d), m));
        }
        Ok(MaskMap(v))
    }

    /// The mask of a `layer/datatype` pair, or `None` when the map does not name it.
    pub fn mask(&self, layer: i16, datatype: i16) -> Option<u8> {
        self.0
            .iter()
            .find(|(k, _)| *k == (layer, datatype))
            .map(|(_, m)| *m)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The layer colour shaded for a mask: mask 1 (or no mask) is the colour itself, then
/// alternately darker and lighter, further each step, so neighbouring masks never share a
/// shade and the layer's hue stays recognisable.
pub fn shade(rgb: (u8, u8, u8), mask: Option<u8>) -> (u8, u8, u8) {
    let m = mask.unwrap_or(1);
    let step = (m / 2) as f64; // 0,1 -> 0 (the colour itself); 2,3 -> 1; 4,5 -> 2; ...
    let t = (0.45 * step).min(0.8);
    let mix = |c: u8, to: f64| (c as f64 + (to - c as f64) * t).round() as u8;
    let to = if m.is_multiple_of(2) { 0.0 } else { 255.0 };
    (mix(rgb.0, to), mix(rgb.1, to), mix(rgb.2, to))
}

/// `#rrggbb` for an RGB triple.
pub fn hex(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_parses_and_answers_by_layer_and_datatype() {
        let m = MaskMap::parse("15/0=1, 15/1=2,17/3=3").unwrap();
        assert_eq!(m.mask(15, 0), Some(1));
        assert_eq!(m.mask(15, 1), Some(2));
        assert_eq!(m.mask(17, 3), Some(3));
        assert_eq!(m.mask(15, 2), None, "an unnamed datatype has no mask");
        assert_eq!(m.mask(16, 0), None);
    }

    #[test]
    fn a_malformed_map_is_refused() {
        for bad in ["15=1", "15/0", "15/0=x", "a/0=1", "15/0=0", "15/0=1,15/0=2"] {
            assert!(MaskMap::parse(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(MaskMap::parse("").unwrap().is_empty());
    }

    #[test]
    fn each_mask_has_its_own_shade_and_mask_one_is_the_layer_colour() {
        let c = (78, 121, 167);
        assert_eq!(shade(c, None), c);
        assert_eq!(shade(c, Some(1)), c);
        let s: Vec<_> = (1..=4).map(|m| shade(c, Some(m))).collect();
        for i in 0..s.len() {
            for j in i + 1..s.len() {
                assert_ne!(s[i], s[j], "masks {} and {} share a shade", i + 1, j + 1);
            }
        }
        assert_eq!(hex((0, 15, 255)), "#000fff");
    }
}
