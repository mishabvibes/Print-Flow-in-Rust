//! A small TensorFlow Lite interpreter in pure Rust: the operators a MediaPipe BlazeFace model uses
//! (float32 / float16-weight models), NHWC layout, single input.
//!
//! This exists because the face detector the original app ran through a 11 MB WASM runtime is a
//! ~230 KB convolutional net; interpreting it directly keeps the app pure Rust, offline and free of
//! native libraries, and keeps the eye key points the auto-crop uses.
//!
//! Kernels index with explicit loops over pre-validated shapes (checked once per operator).
#![allow(clippy::needless_range_loop)]

use crate::flat::Table;

const MAX_ELEMS: usize = 64 << 20;

#[derive(Clone, Debug)]
pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl Tensor {
    fn new(shape: Vec<usize>) -> Option<Self> {
        let n = shape.iter().try_fold(1usize, |a, &d| a.checked_mul(d))?;
        (n <= MAX_ELEMS).then(|| Self { data: vec![0.0; n], shape })
    }
}

#[derive(Clone, Debug)]
enum Opts {
    Conv { same: bool, sw: usize, sh: usize, act: u8, dw: usize, dh: usize },
    Depthwise { same: bool, sw: usize, sh: usize, mult: usize, act: u8, dw: usize, dh: usize },
    Pool { same: bool, sw: usize, sh: usize, fw: usize, fh: usize, act: u8, max: bool },
    Add { act: u8 },
    Concat { axis: i32, act: u8 },
    Reshape { shape: Vec<i32> },
    Plain,
}

#[derive(Clone, Debug)]
struct Op {
    code: i32,
    inputs: Vec<i32>,
    outputs: Vec<i32>,
    opts: Opts,
}

#[derive(Clone, Debug)]
struct Meta {
    shape: Vec<usize>,
    constant: Option<Vec<f32>>,
}

pub struct Model {
    tensors: Vec<Meta>,
    ops: Vec<Op>,
    inputs: Vec<i32>,
    outputs: Vec<i32>,
}

pub fn f16_to_f32(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0f32 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    match e {
        0 => s * m * 2f32.powi(-24),
        31 => {
            if m == 0.0 {
                s * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

fn pos(v: i32) -> usize {
    v.max(1) as usize
}

fn parse_opts(code: i32, t: Option<Table<'_>>) -> Opts {
    let get = |i: usize, d: i32| t.map_or(d, |t| t.i32(i, d));
    let get8 = |i: usize, d: u8| t.map_or(d, |t| t.u8(i, d));
    match code {
        3 => Opts::Conv { same: get8(0, 0) == 0, sw: pos(get(1, 1)), sh: pos(get(2, 1)), act: get8(3, 0), dw: pos(get(4, 1)), dh: pos(get(5, 1)) },
        4 => Opts::Depthwise {
            same: get8(0, 0) == 0,
            sw: pos(get(1, 1)),
            sh: pos(get(2, 1)),
            mult: pos(get(3, 1)),
            act: get8(4, 0),
            dw: pos(get(5, 1)),
            dh: pos(get(6, 1)),
        },
        1 | 17 => Opts::Pool {
            same: get8(0, 0) == 0,
            sw: pos(get(1, 1)),
            sh: pos(get(2, 1)),
            fw: pos(get(3, 1)),
            fh: pos(get(4, 1)),
            act: get8(5, 0),
            max: code == 17,
        },
        0 => Opts::Add { act: get8(0, 0) },
        2 => Opts::Concat { axis: get(0, 0), act: get8(1, 0) },
        22 => Opts::Reshape { shape: t.map(|t| t.vec_i32(0)).unwrap_or_default() },
        _ => Opts::Plain,
    }
}

impl Model {
    pub fn parse(buf: &[u8]) -> Option<Model> {
        let root = Table::root(buf)?;
        let codes: Vec<i32> = (0..root.vec_len(1))
            .filter_map(|i| root.vec_table(1, i))
            .map(|c| {
                // Old files carry the code in a byte field that saturates at 127; newer in an int.
                let dep = c.u8(0, 0) as i32;
                let new = c.i32(3, 0);
                dep.max(new)
            })
            .collect();
        let sg = root.vec_table(2, 0)?;
        let buffers: Vec<Table<'_>> = (0..root.vec_len(4)).filter_map(|i| root.vec_table(4, i)).collect();
        let mut tensors = Vec::new();
        for i in 0..sg.vec_len(0) {
            let t = sg.vec_table(0, i)?;
            let shape: Vec<usize> = t.vec_i32(0).iter().map(|&d| d.max(0) as usize).collect();
            let ttype = t.u8(1, 0);
            let bidx = t.i32(2, 0).max(0) as usize;
            let bytes = if bidx == 0 { &[][..] } else { buffers.get(bidx).map_or(&[][..], |b| b.vec_bytes(0)) };
            let constant = if bytes.is_empty() {
                None
            } else {
                Some(match ttype {
                    0 => bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                    1 => bytes.chunks_exact(2).map(|c| f16_to_f32(u16::from_le_bytes([c[0], c[1]]))).collect(),
                    2 => bytes.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32).collect(),
                    4 => bytes.chunks_exact(8).map(|c| i64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]) as f32).collect(),
                    _ => return None, // quantized models are not supported
                })
            };
            tensors.push(Meta { shape, constant });
        }
        let mut ops = Vec::new();
        for i in 0..sg.vec_len(3) {
            let o = sg.vec_table(3, i)?;
            let code = *codes.get(o.i32(0, 0).max(0) as usize)?;
            ops.push(Op { code, inputs: o.vec_i32(1), outputs: o.vec_i32(2), opts: parse_opts(code, o.table(4)) });
        }
        Some(Model { tensors, ops, inputs: sg.vec_i32(1), outputs: sg.vec_i32(2) })
    }

    pub fn input_shape(&self) -> Option<&[usize]> {
        self.tensors.get(*self.inputs.first()? as usize).map(|t| t.shape.as_slice())
    }

    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    /// Output shapes (for locating the regressor and score tensors).
    pub fn output_shapes(&self) -> Vec<Vec<usize>> {
        self.outputs.iter().filter_map(|&o| self.tensors.get(o as usize)).map(|t| t.shape.clone()).collect()
    }

    /// Runs the network. `input` has the model's input shape (NHWC, flattened).
    pub fn run(&self, input: &[f32]) -> Option<Vec<Tensor>> {
        let mut vals: Vec<Option<Tensor>> = self.tensors.iter().map(|_| None).collect();
        let in_idx = *self.inputs.first()? as usize;
        let in_meta = self.tensors.get(in_idx)?;
        if input.len() != in_meta.shape.iter().product::<usize>() {
            return None;
        }
        *vals.get_mut(in_idx)? = Some(Tensor { shape: in_meta.shape.clone(), data: input.to_vec() });

        for op in &self.ops {
            let get = |slot: usize, vals: &Vec<Option<Tensor>>| -> Option<Tensor> {
                let idx = *op.inputs.get(slot)?;
                if idx < 0 {
                    return None;
                }
                let idx = idx as usize;
                if let Some(Some(t)) = vals.get(idx) {
                    return Some(t.clone());
                }
                let m = self.tensors.get(idx)?;
                m.constant.as_ref().map(|c| Tensor { shape: m.shape.clone(), data: c.clone() })
            };
            let out_idx = *op.outputs.first()? as usize;
            let result = match (&op.opts, op.code) {
                (Opts::Conv { same, sw, sh, act, dw, dh }, _) => {
                    conv2d(&get(0, &vals)?, &get(1, &vals)?, get(2, &vals).as_ref(), *same, (*sw, *sh), (*dw, *dh), *act, false, 1)?
                }
                (Opts::Depthwise { same, sw, sh, mult, act, dw, dh }, _) => {
                    conv2d(&get(0, &vals)?, &get(1, &vals)?, get(2, &vals).as_ref(), *same, (*sw, *sh), (*dw, *dh), *act, true, *mult)?
                }
                (Opts::Pool { same, sw, sh, fw, fh, act, max }, _) => pool(&get(0, &vals)?, *same, (*sw, *sh), (*fw, *fh), *act, *max)?,
                (Opts::Add { act }, _) => {
                    let (a, b) = (get(0, &vals)?, get(1, &vals)?);
                    binary_add(&a, &b, *act)?
                }
                (Opts::Concat { axis, act }, _) => {
                    let ts: Option<Vec<Tensor>> = (0..op.inputs.len()).map(|i| get(i, &vals)).collect();
                    concat(&ts?, *axis, *act)?
                }
                (Opts::Reshape { shape }, _) => {
                    let x = get(0, &vals)?;
                    let want: Vec<i32> = if shape.is_empty() { get(1, &vals)?.data.iter().map(|&v| v as i32).collect() } else { shape.clone() };
                    reshape(x, &want)?
                }
                (_, 6) => get(0, &vals)?,
                (_, 14) => map(get(0, &vals)?, |v| 1.0 / (1.0 + (-v).exp())),
                (_, 19) => map(get(0, &vals)?, |v| v.max(0.0)),
                (_, 21) => map(get(0, &vals)?, |v| v.clamp(0.0, 6.0)),
                (_, 54) => {
                    let (x, a) = (get(0, &vals)?, get(1, &vals)?);
                    prelu(x, &a)?
                }
                (_, 34) => {
                    let (x, p) = (get(0, &vals)?, get(1, &vals)?);
                    pad(&x, &p)?
                }
                _ => {
                    log::warn!("tflite: unsupported operator {}", op.code);
                    return None;
                }
            };
            *vals.get_mut(out_idx)? = Some(result);
        }
        self.outputs.iter().map(|&o| vals.get(o as usize).and_then(|v| v.clone())).collect()
    }
}

fn map(mut t: Tensor, f: impl Fn(f32) -> f32) -> Tensor {
    for v in &mut t.data {
        *v = f(*v);
    }
    t
}

fn activate(v: f32, act: u8) -> f32 {
    match act {
        1 => v.max(0.0),
        2 => v.clamp(-1.0, 1.0),
        3 => v.clamp(0.0, 6.0),
        _ => v,
    }
}

fn prelu(mut x: Tensor, a: &Tensor) -> Option<Tensor> {
    let c = *x.shape.last()?;
    if a.data.len() != c && a.data.len() != 1 {
        return None;
    }
    for (i, v) in x.data.iter_mut().enumerate() {
        if *v < 0.0 {
            *v *= a.data.get(if a.data.len() == 1 { 0 } else { i % c })?;
        }
    }
    Some(x)
}

fn binary_add(a: &Tensor, b: &Tensor, act: u8) -> Option<Tensor> {
    let (big, small) = if a.data.len() >= b.data.len() { (a, b) } else { (b, a) };
    if small.data.is_empty() || big.data.len() % small.data.len() != 0 {
        return None;
    }
    let mut out = big.clone();
    // Broadcast over leading dims (small is a suffix of big, e.g. per-channel or identical).
    for (i, v) in out.data.iter_mut().enumerate() {
        *v = activate(*v + small.data.get(i % small.data.len())?, act);
    }
    Some(out)
}

fn reshape(mut x: Tensor, want: &[i32]) -> Option<Tensor> {
    let total = x.data.len();
    let known: usize = want.iter().filter(|&&d| d >= 0).map(|&d| d as usize).product();
    let shape: Vec<usize> = want.iter().map(|&d| if d < 0 { total.checked_div(known).unwrap_or(0) } else { d as usize }).collect();
    (shape.iter().product::<usize>() == total).then(|| {
        x.shape = shape;
        x
    })
}

fn concat(ts: &[Tensor], axis: i32, act: u8) -> Option<Tensor> {
    let first = ts.first()?;
    let rank = first.shape.len() as i32;
    let ax = if axis < 0 { axis + rank } else { axis };
    if ax < 0 || ax >= rank {
        return None;
    }
    let ax = ax as usize;
    let outer: usize = first.shape[..ax].iter().product();
    let mut shape = first.shape.clone();
    shape[ax] = ts.iter().map(|t| t.shape.get(ax).copied().unwrap_or(0)).sum();
    let mut out = Tensor::new(shape)?;
    let mut o = 0;
    for r in 0..outer {
        for t in ts {
            let inner: usize = t.shape[ax..].iter().product();
            let src = t.data.get(r * inner..(r + 1) * inner)?;
            out.data.get_mut(o..o + inner)?.copy_from_slice(src);
            o += inner;
        }
    }
    if act != 0 {
        for v in &mut out.data {
            *v = activate(*v, act);
        }
    }
    Some(out)
}

fn pad(x: &Tensor, p: &Tensor) -> Option<Tensor> {
    let rank = x.shape.len();
    if p.data.len() != rank * 2 {
        return None;
    }
    let before: Vec<usize> = (0..rank).map(|i| p.data[i * 2].max(0.0) as usize).collect();
    let after: Vec<usize> = (0..rank).map(|i| p.data[i * 2 + 1].max(0.0) as usize).collect();
    let shape: Vec<usize> = (0..rank).map(|i| x.shape[i] + before[i] + after[i]).collect();
    let mut out = Tensor::new(shape.clone())?;
    // Generic N-d copy via strides.
    let mut idx = vec![0usize; rank];
    let strides = |s: &[usize]| -> Vec<usize> {
        let mut st = vec![1; s.len()];
        for i in (0..s.len().saturating_sub(1)).rev() {
            st[i] = st[i + 1] * s[i + 1];
        }
        st
    };
    let (ist, ost) = (strides(&x.shape), strides(&shape));
    for i in 0..x.data.len() {
        let mut rem = i;
        for d in 0..rank {
            idx[d] = rem / ist[d];
            rem %= ist[d];
        }
        let o: usize = (0..rank).map(|d| (idx[d] + before[d]) * ost[d]).sum();
        *out.data.get_mut(o)? = x.data[i];
    }
    Some(out)
}

fn out_dim(inp: usize, k: usize, stride: usize, dil: usize, same: bool) -> (usize, usize) {
    let eff = (k - 1) * dil + 1;
    if same {
        let out = inp.div_ceil(stride);
        let total = ((out.saturating_sub(1)) * stride + eff).saturating_sub(inp);
        (out, total / 2)
    } else {
        (if inp >= eff { (inp - eff) / stride + 1 } else { 0 }, 0)
    }
}

/// Dense and depthwise convolution (NHWC, batch 1). Filter: [Cout,kh,kw,Cin] or, depthwise,
/// [1,kh,kw,Cin*mult].
#[allow(clippy::too_many_arguments)]
fn conv2d(
    x: &Tensor,
    f: &Tensor,
    bias: Option<&Tensor>,
    same: bool,
    stride: (usize, usize),
    dil: (usize, usize),
    act: u8,
    depthwise: bool,
    mult: usize,
) -> Option<Tensor> {
    let [n, ih, iw, ic] = <[usize; 4]>::try_from(x.shape.clone()).ok()?;
    let [f0, kh, kw, f3] = <[usize; 4]>::try_from(f.shape.clone()).ok()?;
    if n != 1 || kh == 0 || kw == 0 {
        return None;
    }
    let oc = if depthwise { f3 } else { f0 };
    if depthwise && ic * mult != oc || !depthwise && f3 != ic {
        return None;
    }
    let (oh, pt) = out_dim(ih, kh, stride.1, dil.1, same);
    let (ow, pl) = out_dim(iw, kw, stride.0, dil.0, same);
    let mut out = Tensor::new(vec![1, oh, ow, oc])?;
    if f.data.len() < f0 * kh * kw * f3 || x.data.len() < ih * iw * ic {
        return None;
    }
    let b = |c: usize| bias.and_then(|b| b.data.get(c)).copied().unwrap_or(0.0);
    for oy in 0..oh {
        for ox in 0..ow {
            let obase = (oy * ow + ox) * oc;
            if depthwise {
                for c in 0..oc {
                    out.data[obase + c] = b(c);
                }
            }
            for ky in 0..kh {
                let iy = (oy * stride.1 + ky * dil.1) as isize - pt as isize;
                if iy < 0 || iy >= ih as isize {
                    continue;
                }
                for kx in 0..kw {
                    let ix = (ox * stride.0 + kx * dil.0) as isize - pl as isize;
                    if ix < 0 || ix >= iw as isize {
                        continue;
                    }
                    let xin = &x.data[(iy as usize * iw + ix as usize) * ic..][..ic];
                    if depthwise {
                        let fb = (ky * kw + kx) * f3;
                        for c in 0..ic {
                            let xv = xin[c];
                            for m in 0..mult {
                                let o = c * mult + m;
                                out.data[obase + o] += xv * f.data[fb + o];
                            }
                        }
                    } else {
                        for o in 0..oc {
                            let fw = &f.data[((o * kh + ky) * kw + kx) * ic..][..ic];
                            let mut s = 0.0f32;
                            for c in 0..ic {
                                s += xin[c] * fw[c];
                            }
                            out.data[obase + o] += s;
                        }
                    }
                }
            }
            for c in 0..oc {
                let v = out.data[obase + c] + if depthwise { 0.0 } else { b(c) };
                out.data[obase + c] = activate(v, act);
            }
        }
    }
    Some(out)
}

fn pool(x: &Tensor, same: bool, stride: (usize, usize), k: (usize, usize), act: u8, max: bool) -> Option<Tensor> {
    let [n, ih, iw, c] = <[usize; 4]>::try_from(x.shape.clone()).ok()?;
    if n != 1 {
        return None;
    }
    let (oh, pt) = out_dim(ih, k.1, stride.1, 1, same);
    let (ow, pl) = out_dim(iw, k.0, stride.0, 1, same);
    let mut out = Tensor::new(vec![1, oh, ow, c])?;
    for oy in 0..oh {
        for ox in 0..ow {
            for ch in 0..c {
                let (mut acc, mut cnt) = (if max { f32::NEG_INFINITY } else { 0.0 }, 0u32);
                for ky in 0..k.1 {
                    let iy = (oy * stride.1 + ky) as isize - pt as isize;
                    for kx in 0..k.0 {
                        let ix = (ox * stride.0 + kx) as isize - pl as isize;
                        if iy < 0 || ix < 0 || iy >= ih as isize || ix >= iw as isize {
                            continue;
                        }
                        let v = x.data[(iy as usize * iw + ix as usize) * c + ch];
                        acc = if max { acc.max(v) } else { acc + v };
                        cnt += 1;
                    }
                }
                let v = if max { acc } else { acc / cnt.max(1) as f32 };
                out.data[(oy * ow + ox) * c + ch] = activate(if cnt == 0 { 0.0 } else { v }, act);
            }
        }
    }
    Some(out)
}
