//! The configuration file for `openrpc-gen`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// Contains information how primitives should be represented.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Primitives {
    /// The name of the type that should be used to represent integers.
    ///
    /// **Default:** `i64`
    #[serde(default = "defaults::integer")]
    pub integer: String,
    /// The name of the type that should be used to represent numbers.
    ///
    /// **Default:** `f64`
    #[serde(default = "defaults::number")]
    pub number: String,
    /// The name of the type that should be used to represent arrays.
    ///
    /// The string `{}` is replaced by the type of the array's items.
    ///
    /// **Default:** `Vec<{}>`
    #[serde(default = "defaults::array")]
    pub array: String,
    /// The name of the type that should be used to represent strings.
    ///
    /// **Default:** `String`
    #[serde(default = "defaults::string")]
    pub string: String,
    /// The name of the type that should be used to represent null values.
    ///
    /// **Default:** `()`
    #[serde(default = "defaults::null")]
    pub null: String,
    /// The name of the type that should be used to represent booleans.
    ///
    /// **Default:** `bool`
    #[serde(default = "defaults::boolean")]
    pub boolean: String,
    /// The name of the type that should be used to represent optional values.
    ///
    /// The string `{}` is replaced by the type of the optional value.
    #[serde(default = "defaults::optional")]
    pub optional: String,
}

impl Default for Primitives {
    fn default() -> Self {
        Self {
            integer: defaults::integer(),
            number: defaults::number(),
            array: defaults::array(),
            string: defaults::string(),
            null: defaults::null(),
            boolean: defaults::boolean(),
            optional: defaults::optional(),
        }
    }
}

/// A collection of formatters used for types with special encoding.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Formatters {
    /// The name of a module that should be used when formatting integers as hexadecimal strings.
    #[serde(default = "defaults::num_as_hex")]
    pub num_as_hex: String,
}

impl Default for Formatters {
    fn default() -> Self {
        Self {
            num_as_hex: defaults::num_as_hex(),
        }
    }
}

/// A list of fixes that should be applied to the parsed file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Fixes {
    /// Whether enum names should be stripped automatically if they are prefixed or suffixed with
    /// a common string.
    ///
    /// **Default:** `false`
    #[serde(default)]
    pub strip_enum_variants: bool,
    /// Flatten fields into their parent structs. Only works on fields that are already
    /// flattened in the OpenRPC document.
    ///
    /// **Default:** `[]`
    #[serde(default)]
    pub flatten: Vec<String>,
    /// Automatically flatten fields that reference a struct with a single field.
    ///
    /// **Default:** `true`
    #[serde(default = "defaults::yes")]
    pub auto_flatten_one_fields: bool,
    /// Automatically flatten fields that are the only referent of their type.
    ///
    /// **Default:** `true`
    #[serde(default = "defaults::yes")]
    pub auto_flatten_one_ref: bool,
    /// A list of symbols to remove from the generated file.
    ///
    /// Be careful, removed symbols are not replaced in the generated code, meaning that
    /// this might create broken references.
    ///
    /// **Default:** `[]`
    #[serde(default)]
    pub remove: Vec<String>,
    /// A list of symbols to rename in the generated file.
    ///
    /// References to those symbols will be automatically updated.
    ///
    /// **Default:** `{}`
    #[serde(default)]
    pub rename: BTreeMap<String, String>,
    /// A list of types to replace with an external type.
    ///
    /// The symbol will be removed from the generated file, and references to it will be replaced
    /// with the provided type.
    ///
    /// **Default:** `{}`
    #[serde(default)]
    pub replace: BTreeMap<String, String>,
    /// Whether stray types should be removed from the generated file.
    ///
    /// Stray types are types that are not referenced by any other type AND that were not
    /// explicitly declared in the OpenRPC document.
    ///
    /// **Default:** `true`
    #[serde(default = "defaults::yes")]
    pub remove_stray_types: bool,
    /// A list of enums that should be tagged.
    ///
    /// The key is the name of the enum, and the value is the name of the tag.
    ///
    /// **Default:** `{}`
    #[serde(default)]
    pub tagged_enums: BTreeMap<String, String>,
    /// Make a specific field a keyword with the specified value.
    ///
    /// This is useful if you have a field which is a String but the specification doesn't
    /// specifically say which value it will have.
    ///
    /// **Default:** `{}`
    #[serde(default)]
    pub make_keyword: BTreeMap<String, String>,
}

impl Default for Fixes {
    fn default() -> Self {
        Self {
            strip_enum_variants: false,
            flatten: Vec::new(),
            remove: Vec::new(),
            rename: BTreeMap::new(),
            replace: BTreeMap::new(),
            remove_stray_types: true,
            auto_flatten_one_fields: true,
            tagged_enums: BTreeMap::new(),
            auto_flatten_one_ref: true,
            make_keyword: BTreeMap::new(),
        }
    }
}

/// Optional Generation.
#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Generation {
    /// A collection of additional `use` statements.
    #[serde(default)]
    pub additional_imports: Vec<String>,
    /// An optional prefix to remove from method names before generating Rust identifiers
    /// from them.
    #[serde(default)]
    pub method_name_prefix: Option<String>,
    /// Whether to generate constants for method names.
    #[serde(default)]
    pub method_name_constants: bool,
    /// Whether to generate type aliases for method result types.
    #[serde(default)]
    pub result_types: bool,
    /// Whether to generate struct types for method parameters.
    #[serde(default)]
    pub param_types: bool,
}

/// The configuration file of `openrpc-gen`. Should be parsed from a TOML file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// The type to use for integers.
    #[serde(default)]
    pub primitives: Primitives,
    /// The fixes that should be applied to the parsed file.
    #[serde(default)]
    pub fixes: Fixes,
    /// Some optional generation options.
    #[serde(default)]
    pub generation: Generation,
    /// The formatters that should be used for types with special encoding.
    #[serde(default)]
    pub formatters: Formatters,
    /// Whether the path of symbols should be written as comments in the generated code.
    ///
    /// **Default:** `false`
    #[serde(default)]
    pub debug_path: bool,
}

/// Loads the configuration file from the provided path.
///
/// Errors are simply returned as strings.
pub fn load(path: &Path) -> Result<Config, String> {
    let contents = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let config = toml::from_str(&contents).map_err(|e| e.to_string())?;
    Ok(config)
}

/// Contains the default values for the configuration.
mod defaults {
    pub fn integer() -> String {
        "i64".into()
    }

    pub fn number() -> String {
        "f64".into()
    }

    pub fn array() -> String {
        "Vec<{}>".into()
    }

    pub fn string() -> String {
        "String".into()
    }

    pub fn null() -> String {
        "()".into()
    }

    pub fn boolean() -> String {
        "bool".into()
    }

    pub fn optional() -> String {
        "Option<{}>".into()
    }

    pub fn yes() -> bool {
        true
    }

    pub fn num_as_hex() -> String {
        "num_as_hex".into()
    }
}
