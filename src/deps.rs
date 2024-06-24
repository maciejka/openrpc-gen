use convert_case::{Case, Casing};
use petgraph::algo;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::hash_map::Keys;
use std::collections::HashMap;

use crate::parse::{TypeKind, TypeRef};

pub(crate) struct TypeDeps {
    g: DiGraph<(), ()>,
    nodes: HashMap<String, NodeIndex<u32>>,
}

impl TypeDeps {
    pub(crate) fn new() -> TypeDeps {
        TypeDeps {
            g: Default::default(),
            nodes: Default::default(),
        }
    }

    pub(crate) fn add(&mut self, config: &crate::config::Config, file: &crate::parse::File) {
        fn lift_out<'a>(
            file: &'a crate::parse::File,
            config: &'a crate::config::Config,
            r: &'a TypeRef,
        ) -> &'a str {
            match r {
                TypeRef::Array(inner) => lift_out(file, config, inner),
                TypeRef::Boolean => &config.primitives.boolean,
                TypeRef::Integer { .. } => &config.primitives.integer,
                TypeRef::Null => &config.primitives.null,
                TypeRef::Number => &config.primitives.number,
                TypeRef::String => &config.primitives.string,
                TypeRef::Keyword(_) => &config.primitives.string.as_str(),
                TypeRef::Ref(path) => match file.types.get(path) {
                    Some(ty) => &ty.name,
                    None => "BrokenReference",
                },
                TypeRef::ExternalRef(name) => name,
            }
        }

        fn add_dep(
            deps: &mut TypeDeps,
            file: &crate::parse::File,
            config: &crate::config::Config,
            a: String,
            b: &TypeRef,
            _required: bool,
        ) {
            let t = lift_out(file, config, b);
            deps.add_edge(a.clone(), String::from(t));
            // if !required {
            //     deps.add_edge(a.clone(), format!("_AddDefault<{}>", t));
            // }
        }

        for ty in file.types.values() {
            match &ty.kind {
                TypeKind::Alias(alias) => {
                    add_dep(self, file, config, ty.name.clone(), &alias.ty, false)
                }
                TypeKind::Struct(s) => {
                    for field in s.fields.values() {
                        add_dep(
                            self,
                            file,
                            config,
                            ty.name.clone(),
                            &&field.ty,
                            field.required,
                        );
                    }
                }
                TypeKind::Enum(e) => {
                    for variant in e.variants.values() {
                        if let Some(inner) = &variant.ty {
                            add_dep(self, file, config, ty.name.clone(), inner, false);
                        }
                    }
                }
            }
        }

        for method in &file.methods {
            let ident_base = if let Some(ref prefix) = config.generation.method_name_prefix {
                method.name.strip_prefix(prefix).unwrap_or(&method.name)
            } else {
                &method.name
            };

            if config.generation.param_types {
                for param in &method.params {
                    add_dep(
                        self,
                        file,
                        config,
                        format!("{}Params", ident_base.to_case(Case::Pascal)),
                        &param.ty,
                        param.required,
                    );
                }
            }
        }
    }

    pub(crate) fn get_nodes(&self) -> Keys<String, NodeIndex<u32>> {
        self.nodes.keys()
    }

    pub(crate) fn has_path(&self, a: &String, b: &String) -> bool {
        if a == b {
            return false;
        }
        if !self.nodes.contains_key(a) || !self.nodes.contains_key(b) {
            return false;
        }
        let a_node = self.nodes.get(a).unwrap();
        let b_node = self.nodes.get(b).unwrap();
        algo::has_path_connecting(&self.g, *a_node, *b_node, None)
    }

    pub(crate) fn has_indirect_path(&self, a: &String, b: &String) -> bool {
        let a_node = self.nodes.get(a).unwrap();
        let b_node = self.nodes.get(b).unwrap();
        let paths =
            algo::k_shortest_path::k_shortest_path(&self.g, *a_node, Some(*b_node), 1, |_| 1);

        if let Some(path) = paths.get(b_node) {
            return *path > 1;
        }

        return false;
    }

    pub(crate) fn add_edge(&mut self, a: String, b: String) {
        let a = match self.nodes.get(&a) {
            Some(n) => *n,
            None => {
                let n = self.g.add_node(());
                self.nodes.insert(a, n);
                n
            }
        };
        let b = match self.nodes.get(&b) {
            Some(n) => *n,
            None => {
                let n = self.g.add_node(());
                self.nodes.insert(b, n);
                n
            }
        };
        self.g.add_edge(a, b, ());
    }
}
