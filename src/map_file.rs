//! Development-only JSON map I/O. No serializer dependency enters either binary.
#[allow(unused_imports)]
use crate::prelude::*;
use crate::{
    error::{Error, Result, bail},
    world::{Block, Elevation, Hall, Map, Spawn},
};
#[derive(Debug)]
enum Value {
    Null,
    Bool,
    Number(String),
    String,
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}
struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn whitespace(&mut self) {
        while self
            .text
            .get(self.at)
            .is_some_and(|c| c.is_ascii_whitespace())
        {
            self.at += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.whitespace();
        if self.text.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn string(&mut self) -> Result<String> {
        if !self.take(b'"') {
            bail!("Expected JSON string");
        }
        let mut out = Vec::new();
        while let Some(&c) = self.text.get(self.at) {
            self.at += 1;
            match c {
                b'"' => {
                    return String::from_utf8(out).map_err(|_| Error::new("Invalid UTF-8".into()));
                }
                b'\\' => {
                    let escape = *self
                        .text
                        .get(self.at)
                        .ok_or_else(|| Error::new("Incomplete escape".into()))?;
                    self.at += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => out.push(escape),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let value = self.hex()?;
                            let n = if (0xd800..=0xdbff).contains(&value) {
                                if self.text.get(self.at..self.at + 2) != Some(b"\\u") {
                                    bail!("Missing surrogate");
                                }
                                self.at += 2;
                                let lo = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&lo) {
                                    bail!("Invalid surrogate");
                                }
                                0x10000 + ((value - 0xd800) << 10) + (lo - 0xdc00)
                            } else {
                                value
                            };
                            let c = char::from_u32(n)
                                .ok_or_else(|| Error::new("Invalid Unicode escape".into()))?;
                            let mut bytes = [0; 4];
                            out.extend(c.encode_utf8(&mut bytes).as_bytes());
                        }
                        _ => bail!("Invalid JSON escape"),
                    }
                }
                0..=31 => bail!("Control byte in string"),
                _ => out.push(c),
            }
        }
        bail!("Unterminated string")
    }
    fn hex(&mut self) -> Result<u32> {
        let bytes = self
            .text
            .get(self.at..self.at + 4)
            .ok_or_else(|| Error::new("Incomplete Unicode escape".into()))?;
        self.at += 4;
        u32::from_str_radix(core::str::from_utf8(bytes).unwrap_or(""), 16)
            .map_err(|_| Error::new("Invalid Unicode escape".into()))
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        if depth > 32 {
            bail!("JSON nesting too deep");
        }
        self.whitespace();
        match self.text.get(self.at).copied() {
            Some(b'"') => {
                self.string()?;
                Ok(Value::String)
            }
            Some(b'{') => {
                self.at += 1;
                let mut values = Vec::new();
                if self.take(b'}') {
                    return Ok(Value::Object(values));
                }
                loop {
                    let key = self.string()?;
                    if !self.take(b':') {
                        bail!("Missing colon");
                    }
                    let value = self.value(depth + 1)?;
                    if values.iter().any(|(k, _)| k == &key) {
                        bail!("Duplicate field {key}");
                    }
                    values.push((key, value));
                    if self.take(b'}') {
                        break;
                    }
                    if !self.take(b',') {
                        bail!("Missing comma");
                    }
                }
                Ok(Value::Object(values))
            }
            Some(b'[') => {
                self.at += 1;
                let mut values = Vec::new();
                if self.take(b']') {
                    return Ok(Value::Array(values));
                }
                loop {
                    values.push(self.value(depth + 1)?);
                    if self.take(b']') {
                        break;
                    }
                    if !self.take(b',') {
                        bail!("Missing comma");
                    }
                }
                Ok(Value::Array(values))
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.at;
                while self
                    .text
                    .get(self.at)
                    .is_some_and(|b| matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
                {
                    self.at += 1;
                }
                let number = core::str::from_utf8(&self.text[start..self.at]).unwrap();
                let n = number
                    .parse::<f64>()
                    .map_err(|_| Error::new("Invalid number".into()))?;
                if !n.is_finite() {
                    bail!("Nonfinite number");
                }
                Ok(Value::Number(number.into()))
            }
            Some(b'n') if self.text.get(self.at..self.at + 4) == Some(b"null") => {
                self.at += 4;
                Ok(Value::Null)
            }
            Some(b't') if self.text.get(self.at..self.at + 4) == Some(b"true") => {
                self.at += 4;
                Ok(Value::Bool)
            }
            Some(b'f') if self.text.get(self.at..self.at + 5) == Some(b"false") => {
                self.at += 5;
                Ok(Value::Bool)
            }
            _ => bail!("Invalid JSON at byte {}", self.at),
        }
    }
}
impl Value {
    fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn required(&self, key: &str) -> Result<&Self> {
        self.get(key)
            .ok_or_else(|| Error::new(format!("Missing {key}")))
    }
    fn number<T: core::str::FromStr>(&self) -> Result<T> {
        if let Self::Number(n) = self {
            n.parse()
                .map_err(|_| Error::new("Invalid numeric field".into()))
        } else {
            bail!("Expected number")
        }
    }
    fn default<T: core::str::FromStr>(&self, key: &str, default: T) -> Result<T> {
        self.get(key).map_or(Ok(default), Self::number)
    }
    fn array(&self) -> Result<&[Self]> {
        if let Self::Array(a) = self {
            Ok(a)
        } else {
            bail!("Expected array")
        }
    }
    fn list(&self, key: &str) -> Result<&[Self]> {
        self.get(key).map_or(Ok(&[]), Self::array)
    }
    fn vector<T: core::str::FromStr, const N: usize>(&self) -> Result<[T; N]> {
        let a = self.array()?;
        if a.len() != N {
            bail!("Expected {N} components");
        }
        let values: Vec<T> = a.iter().map(Self::number).collect::<Result<_>>()?;
        values
            .try_into()
            .map_err(|_| Error::new("Wrong vector size".into()))
    }
}
pub fn load(bytes: &[u8]) -> Result<Map> {
    let mut r = Reader { text: bytes, at: 0 };
    let v = r.value(0)?;
    r.whitespace();
    if r.at != bytes.len() {
        bail!("Trailing JSON data");
    }
    let spawn = v.required("spawn")?;
    Ok(Map {
        version: v.required("version")?.number()?,
        walls: {
            let width: usize = v.required("width")?.number()?;
            let height: usize = v.required("height")?.number()?;
            let bits: Vec<u64> = v
                .required("walls")?
                .array()?
                .iter()
                .map(Value::number)
                .collect::<Result<_>>()?;
            if !(3..=256).contains(&width)
                || !(3..=256).contains(&height)
                || bits.len() != (width * height).div_ceil(64)
            {
                bail!("Invalid bitmap dimensions");
            }
            crate::bitmap::Walls {
                width,
                height,
                bits,
            }
        },
        spawn: Spawn {
            x: spawn.required("x")?.number()?,
            z: spawn.required("z")?.number()?,
            yaw: spawn.default("yaw", 0.0)?,
        },
        exit: v.required("exit")?.vector()?,
        tile_size: v.default("tile_size", 2.0)?,
        ceiling: v.default("ceiling", 3.4)?,
        eye_height: v.default("eye_height", 1.55)?,
        seed: v.default("seed", 0)?,
        elevations: v
            .list("elevations")?
            .iter()
            .map(|e| {
                Ok(Elevation {
                    cell: e.required("cell")?.vector()?,
                    height: e.default("height", 0.0)?,
                    slope: e
                        .get("slope")
                        .map(Value::vector)
                        .transpose()?
                        .unwrap_or([0.0; 2]),
                })
            })
            .collect::<Result<_>>()?,
        blocks: v
            .list("blocks")?
            .iter()
            .map(|b| {
                Ok(Block {
                    min: b.required("min")?.vector()?,
                    max: b.required("max")?.vector()?,
                })
            })
            .collect::<Result<_>>()?,
        breakers: v
            .list("breakers")?
            .iter()
            .map(Value::vector)
            .collect::<Result<_>>()?,
        entities: v
            .list("entities")?
            .iter()
            .map(Value::vector)
            .collect::<Result<_>>()?,
        halls: v
            .list("halls")?
            .iter()
            .map(|h| {
                Ok(Hall {
                    min: h.required("min")?.vector()?,
                    size: h.required("size")?.vector()?,
                    light_profile: 7,
                    light_seed: 0,
                    doors: h
                        .required("doors")?
                        .array()?
                        .iter()
                        .map(Value::vector)
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?,
    })
}
pub fn save(map: &Map) -> Vec<u8> {
    // Development exports contain packed u64 wall words, never character rows.
    let elevations = map
        .elevations
        .iter()
        .map(|v| {
            format!(
                "{{\"cell\":{:?},\"height\":{},\"slope\":{:?}}}",
                v.cell, v.height, v.slope
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let blocks = map
        .blocks
        .iter()
        .map(|v| format!("{{\"min\":{:?},\"max\":{:?}}}", v.min, v.max))
        .collect::<Vec<_>>()
        .join(",");
    let halls = map
        .halls
        .iter()
        .map(|v| {
            format!(
                "{{\"min\":{:?},\"size\":{:?},\"doors\":{:?}}}",
                v.min, v.size, v.doors
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"version\":{},\"width\":{},\"height\":{},\"walls\":{:?},\"spawn\":{{\"x\":{},\"z\":{},\"yaw\":{}}},\"exit\":{:?},\"tile_size\":{},\"ceiling\":{},\"eye_height\":{},\"seed\":{},\"elevations\":[{}],\"blocks\":[{}],\"breakers\":{:?},\"entities\":{:?},\"halls\":[{}]}}",map.version,map.width(),map.height(),map.walls.bits,map.spawn.x,map.spawn.z,map.spawn.yaw,map.exit,map.tile_size,map.ceiling,map.eye_height,map.seed,elevations,blocks,map.breakers,map.entities,halls).into_bytes()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_preserves_full_width_seed_and_geometry() {
        let mut m = Map::procedural(u64::MAX - 12).unwrap();
        m.elevations.push(Elevation {
            cell: [3, 4],
            height: 0.2,
            slope: [0.1, 0.0],
        });
        let n = load(&save(&m)).unwrap();
        assert_eq!(n.seed, m.seed);
        assert_eq!(n.walls, m.walls);
        assert_eq!(n.halls.len(), m.halls.len());
        assert_eq!(n.elevations[0].height, 0.2);
    }
    #[test]
    fn rejects_truncation_duplicates_and_trailing_garbage() {
        assert!(load(b"{").is_err());
        assert!(load(b"{\"a\":1,\"a\":2}").is_err());
        let m = Map::generated(17, 5).unwrap();
        let mut bytes = save(&m);
        bytes.push(b'x');
        assert!(load(&bytes).is_err());
    }
}
