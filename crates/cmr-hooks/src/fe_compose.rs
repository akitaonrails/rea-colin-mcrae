//! HD art in the front end (F27): the menus are one 640x480 picture made
//! of blits, enlarged to the screen each frame (`present`). The frame's
//! blits are replayed to know, for each picture pixel, which image drew it
//! last (and which one is under it). While the picture is enlarged, a
//! screen pixel owned by an image with an HD twin is taken from the twin;
//! where the twin is transparent (its own smooth edge), from the image
//! under it. Pixels the blits don't explain (things drawn some other way,
//! like the menu's globe) keep the picture's own, so nothing is ever drawn
//! out of order: backgrounds, overlaps and animation all compose as the
//! game drew them.

/// A rectangle: x, y, width, height.
pub type Rect = [i32; 4];

/// The picture's size.
pub const W: usize = 640;
pub const H: usize = 480;
/// No owner.
pub const NONE: u16 = u16::MAX;

/// 16-bit pixels, `stride` pixels a row.
#[derive(Clone, Copy)]
pub struct Pixels<'a> {
    pub px: &'a [u16],
    pub w: i32,
    pub h: i32,
    pub stride: usize,
}

impl Pixels<'_> {
    fn at(&self, x: i32, y: i32) -> u16 {
        let (x, y) = (x.clamp(0, self.w - 1), y.clamp(0, self.h - 1));
        self.px[y as usize * self.stride + x as usize]
    }
}

/// One blit of the frame.
#[derive(Clone, Copy)]
pub struct Layer<'a> {
    pub dst: Rect,
    /// The source rectangle in the image, and the image.
    pub src: Rect,
    pub image: Pixels<'a>,
    /// The twin and the same rectangle in its pixels.
    pub twin: Option<(Pixels<'a>, Rect)>,
    /// The colour-key value (transparent), for keyed blits.
    pub key: Option<u16>,
    /// Picture point to image and to twin pixels: offset and scale per
    /// axis (`Layer::new` works them out once).
    to_image: [f32; 4],
    to_twin: [f32; 4],
}

impl Layer<'static> {
    /// A layer that draws nothing.
    pub const EMPTY: Layer<'static> = Layer {
        dst: [0; 4],
        src: [0; 4],
        image: Pixels {
            px: &[],
            w: 0,
            h: 0,
            stride: 0,
        },
        twin: None,
        key: None,
        to_image: [0.0; 4],
        to_twin: [0.0; 4],
    };
}

impl<'a> Layer<'a> {
    pub fn new(
        dst: Rect,
        src: Rect,
        image: Pixels<'a>,
        twin: Option<(Pixels<'a>, Rect)>,
        key: Option<u16>,
    ) -> Layer<'a> {
        // x' = a + (x - dx) * k, as an offset and a scale.
        let map = |to: Rect| {
            let (kx, ky) = (
                to[2] as f32 / dst[2].max(1) as f32,
                to[3] as f32 / dst[3].max(1) as f32,
            );
            [
                to[0] as f32 - dst[0] as f32 * kx,
                kx,
                to[1] as f32 - dst[1] as f32 * ky,
                ky,
            ]
        };
        Layer {
            dst,
            src,
            image,
            twin,
            key,
            to_image: map(src),
            to_twin: twin.map_or([0.0; 4], |t| map(t.1)),
        }
    }

    /// The image's pixel shown at picture point (`fx`, `fy`) (nearest).
    fn source(&self, fx: f32, fy: f32) -> u16 {
        let [ox, kx, oy, ky] = self.to_image;
        self.image.at((ox + fx * kx) as i32, (oy + fy * ky) as i32)
    }

    /// The twin's pixel at picture point (`fx`, `fy`), if it has one.
    fn twin(&self, fx: f32, fy: f32) -> Option<u16> {
        let (twin, _) = self.twin?;
        let [ox, kx, oy, ky] = self.to_twin;
        Some(twin.at((ox + fx * kx) as i32, (oy + fy * ky) as i32))
    }

    fn transparent(&self, v: u16) -> bool {
        self.key == Some(v)
    }
}

/// Fills `top` and `under` (W x H) with the layer that drew each picture
/// pixel last and the one before it, from the layers' own pixels; a pixel
/// whose replayed colour isn't the `picture`'s was drawn some other way
/// and gets no owner.
pub fn owners(layers: &[Layer], picture: &[u16], top: &mut [u16], under: &mut [u16]) {
    top.fill(NONE);
    under.fill(NONE);
    for (k, l) in layers.iter().enumerate().take(NONE as usize) {
        let [dx, dy, dw, dh] = l.dst;
        if dw <= 0 || dh <= 0 || l.src[2] <= 0 || l.src[3] <= 0 {
            continue;
        }
        for y in dy.max(0)..(dy + dh).min(H as i32) {
            for x in dx.max(0)..(dx + dw).min(W as i32) {
                let v = l.source(x as f32 + 0.5, y as f32 + 0.5);
                if l.transparent(v) {
                    continue;
                }
                let p = y as usize * W + x as usize;
                under[p] = top[p];
                top[p] = k as u16;
            }
        }
    }
    for p in 0..W * H {
        if top[p] == NONE {
            continue;
        }
        let (x, y) = ((p % W) as f32 + 0.5, (p / W) as f32 + 0.5);
        if layers[top[p] as usize].source(x, y) != picture[p] {
            top[p] = NONE;
            under[p] = NONE;
        }
    }
}

/// The colour at picture point (`fx`, `fy`) (pixel centres at .5): from the
/// owner's twin, else the picture.
pub fn sample(
    layers: &[Layer],
    top: &[u16],
    under: &[u16],
    picture: &[u16],
    fx: f32,
    fy: f32,
) -> u16 {
    let p = (fy as usize).min(H - 1) * W + (fx as usize).min(W - 1);
    let own = picture[p];
    let Some(l) = layers.get(top[p] as usize) else {
        return own;
    };
    let Some(v) = l.twin(fx, fy) else {
        return own;
    };
    if !l.transparent(v) {
        return v;
    }
    // The twin's edge: what is under it.
    match layers.get(under[p] as usize) {
        Some(u) => match u.twin(fx, fy) {
            Some(v) if !u.transparent(v) => v,
            _ => u.source(fx, fy),
        },
        None => own,
    }
}

/// Whether any picture pixel is owned by a layer with a twin.
pub fn any_twin(layers: &[Layer], top: &[u16]) -> bool {
    top.iter()
        .any(|&o| layers.get(o as usize).is_some_and(|l| l.twin.is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(px: &[u16], w: i32, h: i32) -> Pixels<'_> {
        Pixels {
            px,
            w,
            h,
            stride: w as usize,
        }
    }

    #[test]
    fn owners_follow_the_drawing_order_and_the_picture() {
        let back = vec![1u16; W * H];
        let sprite = [0u16, 2, 2, 2]; // 2x2, top-left transparent (key 0)
        let full = [0, 0, 640, 480];
        let layers = [
            Layer::new(full, full, pixels(&back, 640, 480), None, None),
            Layer::new(
                [10, 10, 2, 2],
                [0, 0, 2, 2],
                pixels(&sprite, 2, 2),
                None,
                Some(0),
            ),
        ];
        let mut picture = back.clone();
        picture[10 * W + 11] = 2;
        picture[11 * W + 10] = 2;
        picture[11 * W + 11] = 9; // drawn over by something else
        let (mut top, mut under) = (vec![0; W * H], vec![0; W * H]);
        owners(&layers, &picture, &mut top, &mut under);
        assert_eq!(top[0], 0);
        assert_eq!(top[10 * W + 10], 0); // the sprite's transparent corner
        assert_eq!((top[10 * W + 11], under[10 * W + 11]), (1, 0));
        assert_eq!(top[11 * W + 11], NONE);
    }

    #[test]
    fn twins_are_sampled_and_their_edges_show_what_is_under() {
        let back = vec![1u16; W * H];
        let back_twin = vec![5u16; 4];
        // A 1x1 sprite with a 2x2 twin, half transparent.
        let sprite = [2u16];
        let twin = [0u16, 7, 7, 7];
        let full = [0, 0, 640, 480];
        let layers = [
            Layer::new(
                full,
                full,
                pixels(&back, 640, 480),
                Some((pixels(&back_twin, 2, 2), [0, 0, 2, 2])),
                None,
            ),
            Layer::new(
                [3, 3, 1, 1],
                [0, 0, 1, 1],
                pixels(&sprite, 1, 1),
                Some((pixels(&twin, 2, 2), [0, 0, 2, 2])),
                Some(0),
            ),
        ];
        let mut picture = back.clone();
        picture[3 * W + 3] = 2;
        let (mut top, mut under) = (vec![0; W * H], vec![0; W * H]);
        owners(&layers, &picture, &mut top, &mut under);
        assert!(any_twin(&layers, &top));
        let s = |x, y| sample(&layers, &top, &under, &picture, x, y);
        assert_eq!(s(100.5, 100.5), 5); // the backdrop's twin
        assert_eq!(s(3.75, 3.75), 7); // the sprite's twin
        assert_eq!(s(3.25, 3.25), 5); // its transparent corner: the backdrop's twin
    }
}
