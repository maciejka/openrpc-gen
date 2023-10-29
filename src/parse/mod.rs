//! Defines the data model we want to target with our parser.

mod logic;

use std::collections::BTreeMap;

use crate::config::Config;

pub use self::logic::parse;

/// An error that occurred during parsing.
#[derive(Debug, Clone)]
pub struct ParsingError {
    /// The path at which the error occured.
    pub path: Path,
    /// A message describing the error.
    pub message: String,
}

/// The output file we want to generate.
#[derive(Debug, Clone)]
pub struct File {
    /// The list of methods defined in the OpenRPC document.
    pub methods: Vec<Method>,
    /// The list of types defined in the OpenRPC document.
    pub types: BTreeMap<Path, TypeDef>,
}

/// An OpenRPC method.
#[derive(Debug, Clone)]
pub struct Method {
    /// The name of the method, as defined in the OpenRPC document.
    pub name: String,
    /// Some documentation about the method.
    pub documentation: Option<String>,
    /// The parameter of the method.
    pub params: Vec<MethodParameter>,
    /// The result of the method.
    ///
    /// If `None`, the method is intended to be used as a notification.
    pub result: Option<MethodResult>,
}

/// A path to a resource defined in an OpenRPC document.
pub type Path = std::rc::Rc<str>;

/// The source of type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeSource {
    /// The type is used as a method parameter or result.
    Method,
    /// The type has been declared clearly in the OpenRPC document.
    Declared,
    /// The type has been inferred from an anonymous schema.
    Anonymous,
}

/// A reference to an existing type.
#[derive(Debug, Clone)]
pub enum TypeRef {
    /// A reference to an existing type defined elsewhere in the document.
    ///
    /// This translates to the name of the referenced type.
    Ref(Path),
    /// A reference to an external type not defined in the document.
    ExternalRef(String),
    /// A boolean value.
    ///
    /// This usually translates to `bool`.
    Boolean,
    /// A string value.
    ///
    /// This usually translates to `String` or `Box<str>`.
    String,
    /// A keyword.
    ///
    /// This is a type of string that can only take one value.
    Keyword(String),
    /// An integer.
    ///
    /// This usually translates to `i64` or `i32`.
    Integer {
        /// The integer should be formatted as an hexadecimal string.
        ///
        /// i.e. `0xDEADBEEF`
        format_as_hex: bool,
    },
    /// A number.
    ///
    /// This usually translates to `f64` or `f32`.
    Number,
    /// An array of elements.
    ///
    /// This usually translates to `Vec<T>` or `Box<[T]>` and does not require a type
    /// definition.
    Array(Box<TypeRef>),
    /// An empty value.
    ///
    /// Usually translates to `()`.
    Null,
}

impl TypeRef {
    /// Returns a string naming the referenced type.
    pub fn name(&self) -> &str {
        match self {
            TypeRef::Ref(path) => path.rsplit('/').next().unwrap_or("value"),
            TypeRef::ExternalRef(name) => name,
            TypeRef::Boolean => "boolean",
            TypeRef::String => "string",
            TypeRef::Keyword(val) => val.as_str(),
            TypeRef::Integer { .. } => "integer",
            TypeRef::Number => "number",
            TypeRef::Array(_) => "array",
            TypeRef::Null => "null",
        }
    }

    /// Returns the path of the referenced type, if any.
    pub fn inner_path(&self) -> Option<&Path> {
        match self {
            TypeRef::Array(inner) => inner.inner_path(),
            TypeRef::Ref(path) => Some(path),
            _ => None,
        }
    }

    /// A collection of attributes to add to the type.
    pub fn attributes(&self, config: &Config, file: &File) -> Vec<String> {
        match self {
            TypeRef::Ref(r) => {
                if let Some(ty) = file.types.get(r) {
                    if let TypeKind::Alias(a) = &ty.kind {
                        return a.ty.attributes(config, file);
                    }
                }
            }
            TypeRef::Integer {
                format_as_hex: true,
            } => {
                return vec![format!(
                    "#[serde(with = \"{}\")]",
                    config.formatters.num_as_hex,
                )]
            }
            _ => (),
        }

        Vec::new()
    }
}

/// The result of an OpenRPC method.
#[derive(Debug, Clone)]
pub struct MethodResult {
    /// The type of the result.
    pub ty: TypeRef,
    /// Some documentation about the result.
    pub documentation: Option<String>,
}

/// A parameter of an OpenRPC method.
#[derive(Debug, Clone)]
pub struct MethodParameter {
    /// The name of the parameter.
    pub name: String,
    /// Some documentation about the parameter.
    pub documentation: Option<String>,
    /// The type of the parameter.
    pub ty: TypeRef,
    /// Whether the parameter is required.
    pub required: bool,
}

/// A type definition.
///
/// A list of type definitions is provided by the OpenRPC document.
#[derive(Debug, Clone)]
pub struct TypeDef {
    /// The path at which the type is defined.
    pub path: Path,
    /// The name of the type.
    pub name: String,
    /// Some documentation associated with the type.
    pub documentation: Option<String>,
    /// The source of this type.
    pub source: TypeSource,
    /// The kind of the type.
    pub kind: TypeKind,
}

/// The kind of a type.
#[derive(Debug, Clone)]
pub enum TypeKind {
    /// A struct.
    Struct(StructDef),
    /// An enum.
    Enum(EnumDef),
    /// A newtype.
    Alias(AliasDef),
}

/// A struct definition.
#[derive(Debug, Clone)]
pub struct StructDef {
    /// A collection of tags that have been found in this struct, but that have been previously
    /// removed from the document by a fix.
    ///
    /// This is required because multiple enum types might rely on that field.
    pub tags: BTreeMap<String, String>,
    /// The fields of this struct.
    pub fields: BTreeMap<Path, StructField>,
}

/// A field of a struct.
#[derive(Debug, Clone)]
pub struct StructField {
    /// The path of the struct field.
    pub path: Path,
    /// The name of the field.
    pub name: String,
    /// Some documentation about the field.
    pub documentation: Option<String>,
    /// Whether the field is required to be present.
    ///
    /// When `false`, the type is wrapped in an `Option<T>`.
    pub required: bool,
    /// Whether the JSON representation of the field should be flatten compared to
    /// the Rust representation.
    ///
    /// This usually means adding the `#[serde(flatten)]` attribute to the field.
    pub flatten: bool,
    /// The type of the field.
    pub ty: TypeRef,
    /// The original name of the field, eventually required to rename the field
    /// with `#[serde(rename = "...")]`.`
    pub name_in_json: String,
}

/// An enum definition.
#[derive(Debug, Clone)]
pub struct EnumDef {
    /// The variants of the enum.
    pub variants: BTreeMap<Path, EnumVariant>,
    /// Describes how the enum is represented in JSON.
    pub tag: EnumTag,
}

/// Describes how an enum is represented in JSON.
#[derive(Debug, Clone)]
pub enum EnumTag {
    /// The enum is not tagged.
    ///
    /// The specific variant is determined by the available properties.
    Untagged,
    /// The enum is tagged with a specific property.
    Tagged(String),
    /// If the enum contains content, it is tagged with object properties. Otherwise,
    /// it is tagged as a string.
    Normal,
}

/// A variant of an enum.
#[derive(Debug, Clone)]
pub struct EnumVariant {
    /// The path of the variant.
    pub path: Path,
    /// The name of the variant.
    pub name: String,
    /// The name of the variant in JSON.
    ///
    /// Depending on the [`EnumTag`] of the enum, this may or may not have any
    /// meaning.
    pub name_in_json: Option<String>,
    /// Some documentation about the variant.
    pub documentation: Option<String>,
    /// The type associated with the variant, if any.
    pub ty: Option<TypeRef>,
}

/// An alias definition.
#[derive(Debug, Clone)]
pub struct AliasDef {
    /// The aliased type.
    pub ty: TypeRef,
}
