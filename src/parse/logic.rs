//! The actual parsing logic.

use std::collections::BTreeMap;

use convert_case::{Case, Casing};
use open_rpc as rpc;

use super::{
    AliasDef, EnumDef, EnumTag, EnumVariant, File, Method, MethodParameter, MethodResult,
    ParsingError, Path, StructDef, StructField, TypeDef, TypeKind, TypeRef, TypeSource,
};

/// Some context required when parsing.
struct Ctx<'a> {
    /// The current path within the document.
    ///
    /// This is used to generate the path of the types.
    pub path: String,

    /// A list of types that have been defined in the document, but outside of the
    /// `components/schemas` section, meaning that they do not have a canonical name.
    pub anonymous_types: BTreeMap<Path, TypeDef>,

    /// The document that is being parsed.
    pub doc: &'a rpc::OpenRpc,

    /// A list of errors that have been encountered during parsing.
    pub errors: Vec<ParsingError>,
}

impl<'a> Ctx<'a> {
    /// Creates a new [`Ctx`] instance.
    pub fn new(doc: &'a rpc::OpenRpc) -> Self {
        Self {
            path: String::from("#"),
            anonymous_types: BTreeMap::new(),
            doc,
            errors: Vec::new(),
        }
    }

    /// Pushes a new path to the context.
    pub fn push_path(&mut self, path: &str) {
        self.path.push('/');
        self.path.push_str(path);
    }

    /// Pops the last component of the current path.
    pub fn pop_path(&mut self) {
        let index = self.path.rfind('/').expect("tried to pop the root path");
        self.path.truncate(index);
    }

    /// Returns the current path.
    #[inline]
    pub fn current_path(&self) -> Path {
        Path::from(self.path.as_str())
    }

    /// Registers a type to the context.
    ///
    /// The path of the type is automatically generated from the current path and returned
    /// to the caller.
    pub fn register_type(&mut self, def: TypeDef) {
        self.anonymous_types.insert(def.path.clone(), def);
    }

    /// Adds a new error to the context.
    pub fn add_error(&mut self, message: impl Into<String>) {
        self.errors.push(ParsingError {
            path: self.current_path(),
            message: message.into(),
        });
    }
}

/// Parses a file from an OpenRPC document.
pub fn parse(doc: &rpc::OpenRpc) -> Result<File, Vec<ParsingError>> {
    let mut methods = Vec::new();
    let mut types = BTreeMap::new();

    let mut ctx = Ctx::new(doc);

    parse_methods(&mut ctx, &mut methods, &doc.methods);

    if let Some(ref components) = doc.components {
        ctx.push_path("components");
        parse_schemas(&mut ctx, &mut types, &components.schemas);
        ctx.pop_path();
    }

    assert_eq!(ctx.path, "#");
    if !ctx.errors.is_empty() {
        return Err(ctx.errors);
    }

    types.append(&mut ctx.anonymous_types);

    Ok(File { methods, types })
}

/// Parse the methods specified in the OpenRPC document into a list of [`Method`]s.
fn parse_methods(ctx: &mut Ctx, output: &mut Vec<Method>, methods: &[rpc::RefOr<rpc::Method>]) {
    ctx.push_path("methods");

    for method in methods {
        match method {
            rpc::RefOr::Inline(method) => {
                output.push(parse_method(ctx, method));
            }
            rpc::RefOr::Reference { .. } => {
                ctx.add_error("externally defined methods are not supported");
            }
        }
    }

    ctx.pop_path();
}

/// Parses a method from the OpenRPC document into a [`Method`].
fn parse_method(ctx: &mut Ctx, method: &rpc::Method) -> Method {
    let mut params = Vec::new();

    ctx.push_path(&method.name);
    let name = method.name.clone();
    let documentation = method
        .description
        .clone()
        .or_else(|| method.summary.clone());
    let result = method
        .result
        .as_ref()
        .and_then(|cd| ref_or_content_descriptor(ctx, cd, parse_method_result));
    parse_params(ctx, &mut params, &method.params);
    ctx.pop_path();

    Method {
        name,
        documentation,
        params,
        result,
    }
}

/// Calls the provided function with either a dereferenced [`rpc::ContentDescriptor`] or the
/// inline one.
fn ref_or_content_descriptor<R>(
    ctx: &mut Ctx,
    cd: &rpc::RefOr<rpc::ContentDescriptor>,
    f: impl FnOnce(&mut Ctx, &rpc::ContentDescriptor) -> R,
) -> Option<R> {
    match cd {
        rpc::RefOr::Inline(cd) => Some(f(ctx, cd)),
        rpc::RefOr::Reference { reference } => match ctx.doc.get_content_descriptor(reference) {
            Some(cd) => Some(f(ctx, cd)),
            None => {
                ctx.add_error(format!("reference `{reference}` not found"));
                None
            }
        },
    }
}

/// Parses the parameters of a method.
fn parse_params(
    ctx: &mut Ctx,
    output: &mut Vec<MethodParameter>,
    params: &[rpc::RefOr<rpc::ContentDescriptor>],
) {
    ctx.push_path("params");

    for param in params {
        ref_or_content_descriptor(ctx, param, |ctx, cd| output.push(parse_param(ctx, cd)));
    }

    ctx.pop_path();
}

/// Parses a method parameter.
fn parse_param(ctx: &mut Ctx, param: &rpc::ContentDescriptor) -> MethodParameter {
    ctx.push_path(&param.name);
    let name = param.name.clone();
    let documentation = param.description.clone().or_else(|| param.summary.clone());
    let ty = parse_type_ref(ctx, TypeSource::Method, &param.schema);
    let required = param.required;
    ctx.pop_path();

    MethodParameter {
        name,
        documentation,
        ty,
        required,
    }
}

/// Parses a [`rpc::ContentDescriptor`] into a method result.
fn parse_method_result(ctx: &mut Ctx, result: &rpc::ContentDescriptor) -> MethodResult {
    ctx.push_path("result");
    let ty = parse_type_ref(ctx, TypeSource::Method, &result.schema);
    let documentation = result
        .description
        .clone()
        .or_else(|| result.summary.clone());
    ctx.pop_path();

    MethodResult { ty, documentation }
}

/// Parses the provided schemas into a list of [`TypeDef`]s.
fn parse_schemas(
    ctx: &mut Ctx,
    output: &mut BTreeMap<Path, TypeDef>,
    schemas: &BTreeMap<String, rpc::Schema>,
) {
    ctx.push_path("schemas");

    for (name, schema) in schemas {
        let ty = parse_type(ctx, Some(name), TypeSource::Declared, schema);
        output.insert(ty.path.clone(), ty);
    }

    ctx.pop_path();
}

/// Parses a type from a [`rpc::Schema`] into a [`TypeDef`].
fn parse_type(
    ctx: &mut Ctx,
    name: Option<&str>,
    source: TypeSource,
    schema: &rpc::Schema,
) -> TypeDef {
    ctx.push_path(name.unwrap_or("_anon"));
    let path = ctx.current_path();
    let name = name
        .or(schema.title.as_deref())
        .unwrap_or("Anonymous")
        .to_case(Case::Pascal);
    let documentation = schema.description.clone();
    let kind = parse_type_kind(ctx, &schema.contents);
    ctx.pop_path();

    TypeDef {
        path,
        name,
        documentation,
        source,
        kind,
    }
}

/// Parses a [`rpc::Schema`] into a [`TypeInfo`].
fn parse_type_ref(ctx: &mut Ctx, source: TypeSource, schema: &rpc::Schema) -> TypeRef {
    let ty = parse_type(ctx, None, source, schema);
    if let TypeKind::Alias(alias) = ty.kind {
        alias.ty
    } else {
        let path = ty.path.clone();
        ctx.register_type(ty);
        TypeRef::Ref(path)
    }
}

/// Parses the provided [`rpc::SchemaContents`] into a [`TypeKind`].
fn parse_type_kind(ctx: &mut Ctx, contents: &rpc::SchemaContents) -> TypeKind {
    match contents {
        rpc::SchemaContents::Reference { reference } => {
            if ctx.doc.get_schema(reference).is_none() {
                ctx.add_error(format!("reference `{}` not found", reference));
            }

            TypeKind::Alias(AliasDef {
                ty: TypeRef::Ref(Path::from(reference.as_str())),
            })
        }
        rpc::SchemaContents::Literal(literal) => literal_to_type_kind(ctx, literal),
        rpc::SchemaContents::AllOf { all_of } => parse_flatten_struct(ctx, true, all_of),
        rpc::SchemaContents::AnyOf { any_of } => parse_flatten_struct(ctx, false, any_of),
        rpc::SchemaContents::OneOf { one_of } => parse_enum(ctx, one_of),
    }
}

/// Converts a [`rpc::Literal`] into a [`TypeRef`].
fn literal_to_type_kind(ctx: &mut Ctx, literal: &rpc::Literal) -> TypeKind {
    match literal {
        rpc::Literal::String(lit) => string_literal_to_type_kind(ctx, lit),
        rpc::Literal::Boolean => TypeKind::Alias(AliasDef {
            ty: TypeRef::Boolean,
        }),
        rpc::Literal::Integer(_) => TypeKind::Alias(AliasDef {
            ty: TypeRef::Integer {
                format_as_hex: false,
            },
        }),
        rpc::Literal::Number(_) => TypeKind::Alias(AliasDef {
            ty: TypeRef::Number,
        }),
        rpc::Literal::Null => TypeKind::Alias(AliasDef { ty: TypeRef::Null }),
        rpc::Literal::Array(lit) => array_literal_to_type_kind(ctx, lit),
        rpc::Literal::Object(lit) => object_literal_to_type_kind(ctx, lit),
    }
}

fn string_literal_to_type_kind(ctx: &mut Ctx, literal: &rpc::StringLiteral) -> TypeKind {
    if let Some(ref e) = literal.enumeration {
        if e.len() == 1 {
            TypeKind::Alias(AliasDef {
                ty: TypeRef::Keyword(e[0].clone()),
            })
        } else {
            TypeKind::Enum(EnumDef {
                variants: e
                    .iter()
                    .map(|e| {
                        ctx.push_path(e);
                        let name = e.to_case(Case::Pascal);
                        let path = ctx.current_path();
                        let out = EnumVariant {
                            path: path.clone(),
                            name,
                            name_in_json: Some(e.clone()),
                            documentation: None,
                            ty: None,
                        };
                        ctx.pop_path();

                        (path, out)
                    })
                    .collect(),
                copy: true,
                tag: EnumTag::Normal,
            })
        }
    } else if literal.pattern.as_deref() == Some("^0x[a-fA-F0-9]+$") {
        TypeKind::Alias(AliasDef {
            ty: TypeRef::Integer {
                format_as_hex: true,
            },
        })
    } else {
        TypeKind::Alias(AliasDef {
            ty: TypeRef::String,
        })
    }
}

/// Converts an arbitrary name to a valid Rust field name.
fn field_name(name_in_json: String) -> String {
    if name_in_json == "type" {
        "ty".into()
    } else if name_in_json.contains(char::is_uppercase) {
        name_in_json.to_lowercase()
    } else {
        name_in_json
    }
}

/// Creates a new [`TypeRef`] for the provided object literal.
fn object_literal_to_type_kind(ctx: &mut Ctx, literal: &rpc::ObjectLiteral) -> TypeKind {
    let mut fields = BTreeMap::new();

    for (name, value) in &literal.properties {
        ctx.push_path(name);
        let path = ctx.current_path();
        let documentation = value.description.clone();
        let ty = parse_type_ref(ctx, TypeSource::Anonymous, value);
        let required = literal.required.contains(name);
        let name_in_json = name.clone();
        let name = field_name(name_in_json.clone());
        ctx.pop_path();

        fields.insert(
            path.clone(),
            StructField {
                path,
                name,
                name_in_json,
                documentation,
                required,
                flatten: false,
                ty,
            },
        );
    }

    TypeKind::Struct(StructDef {
        fields,
        tags: BTreeMap::new(),
    })
}

/// Creates a new [`TypeRef`] for the provided array literal.
fn array_literal_to_type_kind(ctx: &mut Ctx, literal: &rpc::ArrayLiteral) -> TypeKind {
    if let Some(ref items) = literal.items {
        TypeKind::Alias(AliasDef {
            ty: TypeRef::Array(Box::new(parse_type_ref(ctx, TypeSource::Anonymous, items))),
        })
    } else {
        ctx.add_error("array literals without `.items` are not supported");
        TypeKind::Alias(AliasDef {
            ty: TypeRef::Array(Box::new(TypeRef::Null)),
        })
    }
}

/// Parses the provided list of schemas into a flatten struct.
fn parse_flatten_struct(ctx: &mut Ctx, required: bool, schemas: &[rpc::Schema]) -> TypeKind {
    if schemas.len() == 1 {
        return TypeKind::Alias(AliasDef {
            ty: parse_type_ref(ctx, TypeSource::Anonymous, &schemas[0]),
        });
    }

    let mut fields = BTreeMap::new();

    for (i, schema) in schemas.iter().enumerate() {
        ctx.push_path(&format!("field{}", i));
        let path = ctx.current_path();
        let documentation = schema.description.clone();
        let ty = parse_type_ref(ctx, TypeSource::Anonymous, schema);
        let name = match schema.title {
            Some(ref title) => field_name(title.to_case(Case::Snake)),
            None => field_name(ty.name().to_case(Case::Snake)),
        };
        let name_in_json = name.clone();
        ctx.pop_path();

        fields.insert(
            path.clone(),
            StructField {
                path,
                name,
                name_in_json,
                documentation,
                required,
                flatten: true,
                ty,
            },
        );
    }

    TypeKind::Struct(StructDef {
        fields,
        tags: BTreeMap::new(),
    })
}

/// Parses the provided list of schemas into an enum.
fn parse_enum(ctx: &mut Ctx, schemas: &[rpc::Schema]) -> TypeKind {
    let mut variants = BTreeMap::new();

    for (i, schema) in schemas.iter().enumerate() {
        ctx.push_path(&format!("variant{}", i));
        let path = ctx.current_path();
        let documentation = schema.description.clone();
        let ty = parse_type_ref(ctx, TypeSource::Anonymous, schema);
        let name = match schema.title {
            Some(ref title) => title.to_case(Case::Pascal),
            None => ty.name().to_case(Case::Pascal),
        };
        ctx.pop_path();

        variants.insert(
            path.clone(),
            EnumVariant {
                path,
                name_in_json: None,
                name,
                documentation,
                ty: Some(ty),
            },
        );
    }

    TypeKind::Enum(EnumDef {
        variants,
        tag: EnumTag::Untagged,
        copy: false,
    })
}
