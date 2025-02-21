//! Reads the OIL intermediate representation produced by Overkill/LGL's exporters
//!
//! According to the deserialiser in Overkill's tools, the format is:
//! ```rs
//! struct Oil {
//!     magic: b"FORM",
//!     total_size: u32,
//!     nodes: [Node]
//! }
//! 
//! struct Node {
//!     type_code: u32,
//!     length: u32,
//!     data: [u8]
//! }
//! ```
pub mod document;


use std::convert::TryInto;
use std::{path::Path, io::Write};
use vek::{Rgb, Vec2, Vec3};

use crate::util::binaryreader::*;

use crate::util::AsHex;
use crate::util::read_helpers::{TryFromBytesError};
use crate::util::parse_helpers;
use pd2tools_macros::{EnumTryFrom, ItemReader, EnumFromData};

struct UnparsedSection<'a> {
    type_code: u32,
    length: usize,
    bytes: &'a [u8]
}

macro_rules! make_chunks {
    ($($name:ident = $tag:literal),+) => {
        #[derive(Debug, EnumTryFrom, ItemReader)]
        #[repr(u32)]
        pub enum ChunkId {
            $($name = $tag),+
        }

        #[derive(EnumFromData)]
        pub enum Chunk {
            $($name($name)),+
        }

        impl std::fmt::Debug for Chunk {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                match self {
                    $(Self::$name(d) => <$name as std::fmt::Debug>::fmt(d, f)),+
                }
            }
        }

        impl Chunk {
            pub fn tag(&self) -> ChunkId {
                match self {
                    $( Self::$name(_) => ChunkId::$name ),*
                }
            }

            pub fn write_data<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
                match self {
                    $( Self::$name(c) => writer.write_item(c)? ),*
                }
                Ok(())
            }
        }
        
        impl<'a> UnparsedSection<'a> {
            fn try_into_chunk(&self) -> (&'a [u8], Result<Chunk, ReadError>) {

                let mut reader = self.bytes;
                let res = match self.type_code {
                    $($tag => {
                        reader.read_item_as::<$name>().map(Chunk::$name)
                    }),+
                    d => Err(ReadError::BadDiscriminant("ChunkId", d as u128))
                };
                (reader, res)
            }
        }
    }
}

make_chunks! {
    SceneInfo1 = 3,
    SceneInfo2 = 12,
    SceneInfo3 = 20,

    Material = 4,
    MaterialsXml = 11,

    Node = 0,
    Geometry = 5,
    Light = 10,
    Camera = 19,
    
    KeyEvents = 21
    
    //PositionController = 1,
    //RotationController = 2,
    //LookatController = 6,
    //ColorController = 7,
    //AttenuationController = 8,
    //MultiplierController = 9,
    //HotspotController = 13,
    //FalloffController = 14,
    //FovController = 15,
    //FarClipController = 16,
    //NearClipController = 17,
    //TargetDistanceController = 18,
    //IkChainController = 22,
    //IkChainTargetController = 23,
    //CompositePositionController = 24,
    //CompositeRotationController = 25
}


#[derive(Debug)]
enum ParseError {
    NoMagic,
    UnexpectedEof,
    BadUtf8,
    SectionTooShort,
    Mysterious
}
macro_rules! trivialer_from {
    ($type:ty, $variant:ident) => {
        impl From<$type> for ParseError {
            fn from(_: $type) -> Self {
                ParseError::$variant
            }
        }
    }
}
trivialer_from!(TryFromBytesError, UnexpectedEof);
trivialer_from!(std::str::Utf8Error, BadUtf8);

struct UnparsedBytes(Vec<u8>);
impl std::fmt::Debug for UnparsedBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ", self.0.len())?;
        if false && self.0.len() > 64 {
            write!(f, "[{}...]", AsHex(&self.0[0..64]))?;
        }
        else {
            write!(f, "[{}]", AsHex(&self.0))?;
        }
        Ok(())
    }
}

#[derive(Debug, ItemReader)]
pub struct SceneInfo1 {
    pub start_time: f64,
    pub end_time: f64,
}

#[derive(Debug, ItemReader)]
pub struct SceneInfo2 {
    pub start_time: f64,
    pub end_time: f64,

    pub author_tag: String,
    pub source_filename: String,
}

#[derive(Debug, Default, ItemReader)]
pub struct SceneInfo3 {
    pub start_time: f64,
    pub end_time: f64,

    pub author_tag: String,
    pub source_filename: String,
    pub scene_type: String,
}

#[derive(Debug, ItemReader)]
pub struct Material {
    pub id: u32,
    pub name: String,
    pub parent_id: u32
}

#[derive(Debug, ItemReader)]
pub struct MaterialsXml {
    pub xml: String
}

#[derive(Debug, ItemReader)]
pub struct Node {
    pub id: u32,
    pub name: String,

    pub transform: vek::Mat4<f64>,
    pub pivot_transform: vek::Mat4<f64>,

    pub parent_id: u32
}

// Can't derive ItemReader, we have to pass the vertex count in to GeometrySkin.
#[derive(Default, Debug, Clone)]
pub struct Geometry {
    pub node_id: u32,

    /// ID of mesh material
    /// 0xFFFFFFFF == none
    pub material_id: u32,
    pub casts_shadows: u8,
    pub receives_shadows: u8,
    pub channels: Vec<GeometryChannel>,
    pub faces: Vec<GeometryFace>,
    pub skin: Option<GeometrySkin>,
    pub override_bounding_box: Option<BoundingBox>,
}
impl ItemReader for Geometry {
    type Error = ReadError;
    type Item = Geometry;

    fn read_from_stream<R: ReadExt>(stream: &mut R) -> Result<Self::Item, Self::Error> {
        let mut out = Geometry::default();
        out.node_id = stream.read_item()?;
        out.material_id = stream.read_item()?;
        out.casts_shadows = stream.read_item()?;
        out.receives_shadows = stream.read_item()?;
        out.channels = stream.read_item()?;
        out.faces = stream.read_item()?;

        let has_skin: bool = stream.read_item()?;
        if has_skin {
            let vert_count = out.channels.iter()
                .find_map(|i| match i { 
                    GeometryChannel::Position(_, data) => Some(data.len()), _ => None
                });
            if let Some(vert_count) = vert_count {
                out.skin = Some(GeometrySkin::read_from_stream(stream, vert_count)?);
            }
            else {
                return Err(ReadError::Schema("Skins are only valid on meshes that have vertices"))
            }
        }

        let has_bbox: bool = stream.read_item()?;
        if has_bbox {
            out.override_bounding_box = Some(stream.read_item()?);
        }

        Ok(out)
    }

    fn write_to_stream<W: WriteExt>(stream: &mut W, item: &Self::Item) -> Result<(), Self::Error> {
        stream.write_item(&item.node_id)?;
        stream.write_item(&item.material_id)?;
        stream.write_item(&item.casts_shadows)?;
        stream.write_item(&item.receives_shadows)?;
        stream.write_item(&item.channels)?;
        stream.write_item(&item.faces)?;
        if let Some(skin) = &item.skin {
            stream.write_item(&true)?;
            GeometrySkin::write_to_stream(stream, &skin)?;
        }
        else {
            stream.write_item(&false)?;
        }
        if let Some(override_bounding_box) = item.override_bounding_box {
            stream.write_item(&true)?;
            stream.write_item(&override_bounding_box)?;
        }
        else {
            stream.write_item(&false)?;
        }
        Ok(())
    }
}

// Can't derive ItemReader for this, it depends on passing in the vertex count.
#[derive(Debug, Clone)]
pub struct GeometrySkin {
    pub root_node_id: u32,
    pub postmul_transform: vek::Mat4<f64>,
    pub bones: Vec<SkinBoneEntry>,
    pub weights_per_vertex: u32,
    pub weights: Vec<VertexWeight>,

    /// List of lists of bone IDs.
    pub bonesets: Vec<Vec<u32>>
}
impl GeometrySkin {
    fn read_from_stream<R: ReadExt>(stream: &mut R, vertex_count: usize) -> Result<Self, ReadError> {
        let root_node_id = stream.read_item()?;
        let postmul_transform = stream.read_item()?;
        let bones = stream.read_item()?;
        let weights_per_vertex = stream.read_item()?;
        let weight_count = (weights_per_vertex as usize) * vertex_count;
        let mut weights = Vec::with_capacity(weight_count);
        for _ in 0..weight_count {
            weights.push(stream.read_item()?);
        }
        let bonesets = stream.read_item()?;
        Ok(GeometrySkin{ root_node_id, postmul_transform, bones, weights_per_vertex, weights, bonesets })
    }

    fn write_to_stream<W: WriteExt>(stream: &mut W, item: &Self) -> Result<(), ReadError> {
        stream.write_item(&item.root_node_id)?;
        stream.write_item(&item.postmul_transform)?;
        stream.write_item(&item.bones)?;
        stream.write_item(&item.weights_per_vertex)?;
        for w in &item.weights {
            stream.write_item(w)?;
        }
        stream.write_item(&item.bonesets)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, ItemReader)]
pub struct SkinBoneEntry {
    pub bone_node_id: u32,
    pub premul_transform: vek::Mat4<f64>
}

#[derive(Debug, Clone, Copy, ItemReader)]
pub struct VertexWeight {
    pub bone_id: u32,
    pub weight: f64
}

#[derive(Debug, Clone, Copy, ItemReader)]
pub struct BoundingBox {
    pub min: Vec3<f64>,
    pub max: Vec3<f64>
}

#[derive(Debug, Clone, ItemReader)]
pub enum GeometryChannel {
    #[tag(0)] Position(u32, Vec<Vec3<f64>>),
    #[tag(1)] TexCoord(u32, Vec<Vec2<f64>>),
    #[tag(2)] Normal  (u32, Vec<Vec3<f64>>),
    #[tag(3)] Binormal(u32, Vec<Vec3<f64>>),
    #[tag(4)] Tangent (u32, Vec<Vec3<f64>>),
    #[tag(5)] Colour  (u32, Vec<Rgb<f64>>)
}

#[derive(Debug, Clone, ItemReader)]
pub struct GeometryFace {
    pub material_id: u32,
    pub unknown1: u32,

    pub loops: Vec<GeometryFaceloop>
}

#[derive(Debug, Clone, Copy, ItemReader)]
pub struct GeometryFaceloop {
    pub channel: u32,
    pub a: u32,
    pub b: u32,
    pub c: u32
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, EnumTryFrom, ItemReader)]
#[repr(u32)]
pub enum LightType {
    Spot = 0,
    Directional = 1,
    Omni = 2
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, EnumTryFrom, ItemReader)]
#[repr(u32)]
pub enum SpotlightShape {
    Rectangular = 0,
    Circular = 1
}

#[derive(Debug, ItemReader)]
pub struct Light {
    pub node_id: u32,
    pub lamp_type: LightType,
    pub color: Rgb<f64>,
    pub multiplier: f64,
    pub far_attenuation_end: f64,
    pub far_attenuation_start: f64,
    pub near_attenuation_end: f64,
    pub near_attenuation_start: f64,
    pub falloff: f64,
    pub hotspot: f64,
    pub aspect_ratio: f64,
    pub overshoot: bool,
    pub shape: SpotlightShape,
    pub target_id: u32,
    pub on: bool
}

#[derive(Debug, PartialEq, Clone, Copy, ItemReader,)]
pub struct Camera {
    pub node_id: u32,
    pub fov: f64,
    pub far_clip: f64,
    pub near_clip: f64,
    pub target_id: u32,
    pub target_distance: f64,
    pub aspect_ratio: f64
}

/// "Beats and triggers" block.
#[derive(Debug, ItemReader)]
pub struct KeyEvents {
    pub events: Vec<KeyEvent>
}

#[derive(Debug, ItemReader)]
pub struct KeyEvent {
    pub id: u32,
    pub name: String,
    pub timestamp: f64,
    pub node_id: u32,    // The maya2017 exporter always writes 0xFFFFFFFF,
    pub event_type: String, // Exporter always writes "beat" or "trigger" here
    pub parameter_count: u32     // Exporter always writes 0
}

fn split_to_sections<'a>(mut src: &'a [u8]) -> Result<Vec<UnparsedSection<'a>>, ReadError> {
    let mut out = Vec::<UnparsedSection>::new();

    let magic: [u8; 4] = src.read_item()?;
    if magic != *b"FORM" {
        return Err(ReadError::Schema("No magic number"));
    }

    let total_size = src.read_item_as::<u32>()?;

    while src.len() > 8 {
        let type_code: u32 = src.read_item()?;
        let length: usize = src.read_item_as::<u32>()?.try_into().unwrap();
        if length > src.len() { 
            return Err(ReadError::ItemTooLong(length as usize))
        }
        let (chunk_body, remaining) = src.split_at(length);
        out.push(UnparsedSection {
            type_code,
            length,
            bytes: chunk_body
        });
        src = remaining;
    }

    Ok(out)
}

pub fn print_sections(filename: &Path) {
    let bytes = match std::fs::read(filename) {
        Err(e) => { println!("Error reading {:?}: {}", filename, e); return} 
        Ok(v) => v
    };
    
    let data = match split_to_sections(&bytes) {
        Err(e) => { println!("Error reading {:?}: {:?}", filename, e); return},
        Ok(v) => v
    };

    let mut offset = 8;
    for sec in data {
        print!("{:6} {:6} ", offset, sec.length);
        offset += sec.length;
        let (remain, res) = sec.try_into_chunk();
        match res {
            Ok(chunk) => println!("{:?} {:}", chunk, AsHex(remain)),
            Err(e) => println!("{:4} {:?} {:}", sec.type_code, e, sec.length - remain.len())
        }
    }
}

pub fn chunks_to_bytes(chunks: &[Chunk]) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::<u8>::with_capacity(8*1024*1024);
    buf.write(b"FORM")?;
    buf.write_item(&0xFFFFFFFFu32)?; // This will be the length later;

    for chunk in chunks{
        buf.write_item(&chunk.tag())?;

        let len_pos = buf.len();
        buf.write_item(&0xFFFFFFFFu32)?;

        let start_pos = buf.len();
        chunk.write_data(&mut buf)?;
        let length: u32 = (buf.len() - start_pos).try_into().unwrap();

        (&mut buf[len_pos..]).write_item(&length)?;
    }
    let len: u32 = buf.len().try_into().unwrap();
    buf.write_item(&len)?;
    (&mut buf[4..8]).write_item(&len)?;
    Ok(buf)
}