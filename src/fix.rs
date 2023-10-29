use std::collections::{BTreeMap, BTreeSet};

use crate::config::Config;
use crate::parse::{EnumTag, EnumVariant, File, Path, TypeKind, TypeRef, TypeSource};

/// Fixes the provided file according to the provided configuration.
pub fn fix(file: &mut File, config: &Config) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();

    if config.fixes.strip_enum_variants {
        strip_enum_variants(file);
    }
    make_keywords(file, &config.fixes.make_keyword, &mut errs);
    flatten_fields(file, &config.fixes.flatten, &mut errs);
    if config.fixes.auto_flatten_one_fields {
        flatten_one_fields(file, &mut errs);
    }
    if config.fixes.auto_flatten_one_ref {
        flatten_one_refs(file, &mut errs);
    }
    remove_things(file, &config.fixes.remove, &mut errs);
    replace_types(file, &config.fixes.replace, &mut errs);
    rename_things(file, &config.fixes.rename, &mut errs);
    tag_enums(file, &config.fixes.tagged_enums, &mut errs);
    if config.fixes.remove_stray_types {
        remove_stray_types(file);
    }

    if !errs.is_empty() {
        return Err(errs);
    }

    Ok(())
}

fn strip_enum_variants(file: &mut File) {
    for ty in file.types.values_mut() {
        if let TypeKind::Enum(en) = &mut ty.kind {
            fixup_variants(&mut en.variants);
        }
    }
}

fn fixup_variants(variants: &mut BTreeMap<Path, EnumVariant>) {
    // Fast path: only one variant.
    if variants.len() <= 1 {
        return;
    }

    let common_prefix = common_prefix(variants.values().map(|v| v.name.as_str())).len();
    let common_suffix = common_suffix(variants.values().map(|v| v.name.as_str())).len();

    if common_prefix == common_suffix {
        // All the variants have the same name.
        return;
    }

    for variant in variants.values_mut() {
        variant.name = variant.name[common_prefix..variant.name.len() - common_suffix].into();
    }
}

fn flatten_fields(file: &mut File, paths: &[String], errs: &mut Vec<String>) {
    // The list of paths in `paths` that area types instead of fields.
    // Those must be filtered.
    let mut types = BTreeSet::new();

    // The paths to add to the list.
    let mut paths2 = Vec::new();

    // If some of the paths refer to a type, add that type to the list.
    for ty in file.types.values() {
        let TypeKind::Struct(s) = &ty.kind else {
            continue;
        };
        for field in s.fields.values() {
            let TypeRef::Ref(path) = &field.ty else {
                continue;
            };
            if !paths.iter().any(|x| x == &**path) {
                continue;
            }

            let replaced_type = file.types.get(&**path).unwrap();

            match &replaced_type.kind {
                TypeKind::Alias(_) => (),
                TypeKind::Struct(_) => {
                    if !field.flatten {
                        continue;
                    }
                }
                TypeKind::Enum(_) => continue,
            }

            types.insert(path.clone());
            paths2.push(field.path.clone());
        }
    }

    for path in paths
        .iter()
        .map(|x| &**x)
        .filter(|x| !types.contains(*x))
        .chain(paths2.iter().map(|x| &**x))
    {
        match flatten_field(file, path) {
            Ok(()) => (),
            Err(err) => errs.push(err),
        }
    }
}

fn flatten_one_fields(file: &mut File, errs: &mut Vec<String>) {
    let mut fields = Vec::new();

    for ty in file.types.values() {
        let TypeKind::Struct(s) = &ty.kind else {
            continue;
        };
        for field in s.fields.values() {
            if !field.flatten {
                continue;
            }
            let TypeRef::Ref(r) = &field.ty else {
                continue;
            };
            let Some(target_ty) = file.types.get(r) else {
                continue;
            };
            let TypeKind::Struct(target_s) = &target_ty.kind else {
                continue;
            };
            if target_s.fields.len() != 1 {
                continue;
            }
            fields.push(field.path.clone());
        }
    }

    for field in fields {
        if let Err(err) = flatten_field(file, &field) {
            errs.push(err);
        }
    }
}

fn flatten_one_refs(file: &mut File, errs: &mut Vec<String>) {
    let mut fields = Vec::new();
    let mut aliases = Vec::new();

    for ty in file.types.values() {
        match &ty.kind {
            TypeKind::Struct(s) => {
                for field in s.fields.values() {
                    if !field.flatten {
                        continue;
                    }
                    let TypeRef::Ref(ty_path) = &field.ty else {
                        continue;
                    };
                    if count_refs(file, ty_path) == 1 {
                        fields.push(field.path.clone());
                    }
                }
            }
            TypeKind::Alias(a) => {
                if let TypeRef::Ref(ty_path) = &a.ty {
                    if count_refs(file, ty_path) == 1 {
                        aliases.push(ty.path.clone());
                    }
                }
            }
            _ => (),
        }
    }

    // For fields, we already have a working function.
    for field in fields {
        if let Err(err) = flatten_field(file, &field) {
            errs.push(err);
        }
    }

    // For aliases, we have to replace the whole alias with the aliased struct.
    // This might not be possible for all aliases in the future. One should check for this here.
    for alias in aliases {
        let ty = file.types.get(&alias).unwrap();
        let ty_path = ty.path.clone();
        let TypeKind::Alias(a) = &ty.kind else {
            unreachable!();
        };
        let TypeRef::Ref(r) = &a.ty else {
            unreachable!();
        };
        let replaced_type_path = r.clone();
        let Some(replaced_type) = file.types.remove(&replaced_type_path) else {
            errs.push(format!(
                "\
                can't flatten alias: broken reference found
                - type = {}
                - ref = {}
                ",
                ty_path, replaced_type_path,
            ));
            continue;
        };
        let alias = file.types.get_mut(&alias).unwrap();

        // Right now, the only we need to do when merging the alias with its child is to
        // perserve the alias's path.
        let og_path = alias.path.clone();
        let og_name = alias.name.clone();
        *alias = replaced_type;
        alias.name = og_name;
        alias.path = og_path;
    }
}

fn get_inner_ref(r: &TypeRef) -> Option<&Path> {
    match r {
        TypeRef::Ref(r) => Some(r),
        TypeRef::Array(r) => get_inner_ref(r),
        _ => None,
    }
}

fn count_refs(file: &File, type_path: &str) -> usize {
    let mut count = 0;

    for ty in file.types.values() {
        match &ty.kind {
            TypeKind::Struct(s) => {
                for field in s.fields.values() {
                    if get_inner_ref(&field.ty).is_some_and(|p| &**p == type_path) {
                        count += 1;
                    }
                }
            }
            TypeKind::Enum(e) => {
                for variant in e.variants.values() {
                    if let Some(ty) = &variant.ty {
                        if get_inner_ref(ty).is_some_and(|p| &**p == type_path) {
                            count += 1;
                        }
                    }
                }
            }
            TypeKind::Alias(a) => {
                if get_inner_ref(&a.ty).is_some_and(|p| &**p == type_path) {
                    count += 1;
                }
            }
        }
    }

    for method in &file.methods {
        if let Some(result) = &method.result {
            if get_inner_ref(&result.ty).is_some_and(|p| &**p == type_path) {
                count += 1;
            }
        }

        for param in &method.params {
            if get_inner_ref(&param.ty).is_some_and(|p| &**p == type_path) {
                count += 1;
            }
        }
    }

    count
}

fn flatten_field(file: &mut File, path: &str) -> Result<(), String> {
    let mut found = None;

    for ty in file.types.values() {
        let TypeKind::Struct(s) = &ty.kind else {
            continue;
        };
        let Some(field) = s.fields.get(path) else {
            continue;
        };
        let target_path = match &field.ty {
            TypeRef::Ref(ok) => ok,
            other => {
                return Err(format!(
                    "\
                    can't flatten: field is a primitive:\n\
                    - field = {path}\n\
                    - type  = {other:?}\n\
                    ",
                ));
            }
        };
        found = Some((field.flatten, target_path.clone(), ty.path.clone()));
        break;
    }

    let Some((field_is_flatten, target_type, into_type)) = found else {
        return Err(format!(
            "\
            can't flatten: field or type not found:\n
            - field = {path}\n\
            ",
        ));
    };

    match &file.types.get(&target_type).unwrap().kind {
        TypeKind::Alias(a) => {
            // Just change the type to the referenced type.

            let r = a.ty.clone();

            // Remove the field to flatten.
            let TypeKind::Struct(s) = &mut file.types.get_mut(&into_type).unwrap().kind else {
                unreachable!();
            };

            s.fields.get_mut(path).unwrap().ty = r;
        }
        TypeKind::Struct(target_s) => {
            if !field_is_flatten {
                return Err(format!(
                    "\
                can't flatten: field is not flatten:\n\
                - field = {path}\n\
                ",
                ));
            }

            let mut fields_to_add = target_s.fields.clone();

            // Remove the field to flatten.
            let TypeKind::Struct(s) = &mut file.types.get_mut(&into_type).unwrap().kind else {
                unreachable!();
            };

            s.fields.remove(path);
            s.fields.append(&mut fields_to_add);
        }
        TypeKind::Enum(_) => {
            return Err(format!(
                "\
            can't flatten: target type is not a struct:\n\
            - field       = {path}\n\
            - target_type = {target_type}\n\
            ",
            ))
        }
    }

    Ok(())
}

fn remove_things(file: &mut File, paths: &[String], errs: &mut Vec<String>) {
    for path in paths {
        if !remove_thing(file, path) {
            errs.push(format!(
                "\
                can't remove: path not found:\n\
                - path = {path}\n\
                ",
            ));
        }
    }
}

fn remove_thing(file: &mut File, path: &str) -> bool {
    if file.types.remove(path).is_some() {
        return true;
    }

    for ty in file.types.values_mut() {
        match &mut ty.kind {
            TypeKind::Struct(s) => {
                if s.fields.remove(path).is_some() {
                    return true;
                }
            }
            TypeKind::Enum(e) => {
                if e.variants.remove(path).is_some() {
                    return true;
                }
            }
            TypeKind::Alias(_) => (),
        }
    }

    false
}

fn rename_things(file: &mut File, replacements: &BTreeMap<String, String>, errs: &mut Vec<String>) {
    for (path, by) in replacements {
        if let Err(err) = rename_thing(file, path, by) {
            errs.push(err);
        }
    }
}

fn rename_thing(file: &mut File, path: &str, by: &str) -> Result<(), String> {
    if let Some(ty) = file.types.get_mut(path) {
        ty.name = by.into();
        return Ok(());
    }

    for ty in file.types.values_mut() {
        match &mut ty.kind {
            TypeKind::Struct(s) => {
                if let Some(field) = s.fields.get_mut(path) {
                    field.name = by.into();
                    return Ok(());
                }
            }
            TypeKind::Enum(e) => {
                if let Some(variant) = e.variants.get_mut(path) {
                    variant.name = by.into();
                    return Ok(());
                }
            }
            TypeKind::Alias(_) => (),
        }
    }

    Err(format!(
        "\
        can't rename: path not found:\n\
        - path = {path}\n\
        ",
    ))
}

fn replace_types(file: &mut File, replacements: &BTreeMap<String, String>, errs: &mut Vec<String>) {
    for (path, by) in replacements {
        if !replace_type(file, path, by.into()) {
            errs.push(format!(
                "\
                can't replace: type not found:\n\
                - path = {path}\n\
                ",
            ));
        }
    }
}

fn replace_type(file: &mut File, path: &str, by: String) -> bool {
    if file.types.remove(path).is_none() {
        return false;
    }

    for ty in file.types.values_mut() {
        match &mut ty.kind {
            TypeKind::Struct(s) => {
                for field in s.fields.values_mut() {
                    if matches!(&field.ty, TypeRef::Ref(p) if &**p == path) {
                        field.ty = TypeRef::ExternalRef(by.clone());
                    }
                }
            }
            TypeKind::Enum(e) => {
                for variant in e.variants.values_mut() {
                    if matches!(&variant.ty, Some(TypeRef::Ref(p)) if &**p == path) {
                        variant.ty = Some(TypeRef::ExternalRef(by.clone()));
                    }
                }
            }
            TypeKind::Alias(a) => {
                if matches!(&a.ty, TypeRef::Ref(p) if &**p == path) {
                    a.ty = TypeRef::ExternalRef(by.clone());
                }
            }
        }
    }

    for method in &mut file.methods {
        if let Some(result) = &mut method.result {
            if matches!(&result.ty, TypeRef::Ref(p) if &**p == path) {
                result.ty = TypeRef::ExternalRef(by.clone());
            }
        }

        for param in &mut method.params {
            if matches!(&param.ty, TypeRef::Ref(p) if &**p == path) {
                param.ty = TypeRef::ExternalRef(by.clone());
            }
        }
    }

    true
}

fn remove_stray_types(file: &mut File) {
    // The set of all nodes that are known not be stray types.
    let mut not_stray = BTreeSet::new();
    // Nodes to visit next.
    let mut to_visit = file
        .types
        .values()
        .filter(|ty| ty.source == TypeSource::Method)
        .map(|ty| ty.path.clone())
        .chain(
            file.methods
                .iter()
                .filter_map(|m| m.result.as_ref().and_then(|r| r.ty.inner_path()).cloned()),
        )
        .chain(
            file.methods
                .iter()
                .flat_map(|m| m.params.iter().filter_map(|p| p.ty.inner_path()).cloned()),
        )
        .collect::<Vec<_>>();

    fn take_ref_into_account(r: &TypeRef, to_visit: &mut Vec<Path>) {
        if let Some(r) = r.inner_path() {
            to_visit.push(r.clone());
        }
    }

    // Visit the graph to find all the nodes that are not stray types.
    while let Some(path) = to_visit.pop() {
        if !not_stray.insert(path.clone()) {
            continue;
        }

        let ty = match file.types.get(&path) {
            Some(ty) => ty,

            // This branch can be taken if the user has removed a type that's
            // still referenced by another type.
            None => continue,
        };

        match &ty.kind {
            TypeKind::Struct(s) => {
                for field in s.fields.values() {
                    take_ref_into_account(&field.ty, &mut to_visit);
                }
            }
            TypeKind::Enum(e) => {
                for variant in e.variants.values() {
                    if let Some(r) = &variant.ty {
                        take_ref_into_account(r, &mut to_visit);
                    }
                }
            }
            TypeKind::Alias(r) => {
                take_ref_into_account(&r.ty, &mut to_visit);
            }
        }
    }

    file.types.retain(|_, ty| not_stray.contains(&ty.path));
}

fn tag_enums(file: &mut File, tagged: &BTreeMap<String, String>, errs: &mut Vec<String>) {
    for (path, tag) in tagged {
        if let Err(err) = tag_enum(file, Path::from(&**path), tag) {
            errs.push(err);
        }
    }
}

fn tag_enum(file: &mut File, path: Path, tag: &str) -> Result<(), String> {
    let Some(ty) = file.types.get(&path) else {
        return Err(format!(
            "\
            failed to tag enum: path not found\n\
            - path = {path}\n\
            "
        ));
    };
    let TypeKind::Enum(e) = &ty.kind else {
        return Err(format!(
            "\
            failed to tag enum: type is not an enum
            - path = {path}\n\
            "
        ));
    };
    let mut to_fix = Vec::new();
    for variant in e.variants.values() {
        let Some(inner) = &variant.ty else {
            return Err(format!(
                "\
                failed to tag enum: variant contains no inner value
                - path = {path}
                - variant = {}
                ",
                variant.name
            ));
        };
        let TypeRef::Ref(r) = &inner else {
            return Err(format!(
                "\
                failed to tag enum: inner type is a literal
                - path = {path}
                - variant = {}
                - type = {:?}
                ",
                variant.name, inner
            ));
        };
        match find_keyword(file, r.clone(), tag) {
            Ok(res) => to_fix.push((variant.path.clone(), res)),
            Err(err) => return Err(err),
        }
    }

    // Fix the stuff now that we have mutable access to the file.
    let enum_ty = file.types.get_mut(&path).unwrap();
    let TypeKind::Enum(e) = &mut enum_ty.kind else {
        unreachable!();
    };
    e.tag = EnumTag::Tagged(tag.to_owned());
    for (var_path, res) in &to_fix {
        e.variants.get_mut(var_path).unwrap().name_in_json = Some(res.value.clone());
    }

    for (_, res) in &to_fix {
        for (ty_path, field_path) in &res.fields {
            let ty = file.types.get_mut(ty_path).unwrap();
            let TypeKind::Struct(s) = &mut ty.kind else {
                unreachable!()
            };
            s.fields.remove(field_path);
            s.tags.insert(tag.to_owned(), res.value.clone());
        }
    }

    Ok(())
}

/// The result of looking for a keyword field.
#[derive(Debug)]
struct FindKeywordResult {
    /// The fields that have been found with the requested name.
    ///
    /// All those fields have the same keyword value.
    pub fields: Vec<(Path, Path)>,
    /// The value of the keyword.
    pub value: String,
}

/// Attempts to recursively find a field with the given name.
///
/// # Arguments
///
/// - `file`: The [`File`] structure being analyzed.
///
/// - `path`: The path to the struct to analyze. [`None`] is returned if the type is not a struct.
///
/// - `name`: The name of the field.
fn find_keyword(file: &File, path: Path, name: &str) -> Result<FindKeywordResult, String> {
    let Some(ty) = file.types.get(&path) else {
        return Err(format!(
            "\
            failed to tag enum: path not found\n\
            - path = {path}\n\
            "
        ));
    };
    match &ty.kind {
        TypeKind::Struct(s) => {
            if let Some(value) = s.tags.get(name) {
                return Ok(FindKeywordResult {
                    fields: Vec::new(),
                    value: value.clone(),
                });
            }

            // For structs, we need to find exactly one field that has the keyword.
            let mut ret = None;

            for field in s.fields.values() {
                if field.name_in_json == name {
                    let TypeRef::Keyword(value) = &field.ty else {
                        return Err(format!(
                            "\
                            failed to tag enum: field is not a keyword\n\
                            - path = {path}\n\
                            - field = {field}\n\
                            - name = {name}\n\
                            ",
                            field = field.path,
                        ));
                    };

                    if ret.is_some() {
                        return Err(format!(
                            "\
                            failed to tag enum: multiple fields with the same name found\n\
                            - path = {path}\n\
                            - name = {name}\n\
                            ",
                        ));
                    }

                    ret = Some(FindKeywordResult {
                        fields: vec![(path.clone(), field.path.clone())],
                        value: value.clone(),
                    });
                }

                if !field.flatten {
                    continue;
                }

                let TypeRef::Ref(r) = &field.ty else {
                    continue;
                };

                let ret2 = find_keyword(file, r.clone(), name);

                if ret.is_some() && ret2.is_ok() {
                    return Err(format!(
                        "\
                        failed to tag enum: multiple fields with the same name found\n\
                        - path = {path}\n\
                        - name = {name}\n\
                        ",
                    ));
                } else if ret2.is_ok() {
                    ret = Some(ret2.unwrap());
                }
            }

            if let Some(ret) = ret {
                Ok(ret)
            } else {
                Err(format!(
                    "\
                    failed to tag enum: no field with the requested name found in struct fields\n\
                    - path = {path}\n\
                    - name = {name}\n\
                    ",
                ))
            }
        }
        TypeKind::Enum(e) => {
            if matches!(e.tag, EnumTag::Normal) {
                return Err(format!(
                    "\
                    failed to tag enum: enum is already tagged\n\
                    - path = {path}\n\
                    ",
                ));
            }

            // For enums, all variants must have the same value.
            let mut rets = Vec::new();
            let mut value = None;

            for variant in e.variants.values() {
                let Some(ty) = &variant.ty else {
                    return Err(format!(
                        "\
                        failed to tag enum: variant contains no inner value
                        - path = {path}
                        - variant = {}
                        ",
                        variant.name,
                    ));
                };

                let TypeRef::Ref(r) = ty else {
                    return Err(format!(
                        "\
                        failed to tag enum: inner type is a literal
                        - path = {path}
                        - variant = {}
                        - type = {:?}
                        ",
                        variant.name, ty,
                    ));
                };

                match find_keyword(file, r.clone(), name) {
                    Ok(mut ret) => {
                        match value {
                            Some(ref val) => {
                                if val != &ret.value {
                                    return Err(format!(
                                        "\
                                    failed to tag enum: multiple fields with the same name found\n\
                                    - path = {path}\n\
                                    - name = {name}\n\
                                    ",
                                    ));
                                }
                            }
                            None => value = Some(ret.value.clone()),
                        }

                        rets.append(&mut ret.fields);
                    }
                    Err(err) => return Err(err),
                }
            }

            if value.is_none() {
                return Err(format!(
                    "\
                    failed to tag enum: no field with the requested name found in enum variants\n\
                    - path = {path}\n\
                    - name = {name}\n\
                    ",
                ));
            }

            Ok(FindKeywordResult {
                fields: rets,
                value: value.unwrap(),
            })
        }
        TypeKind::Alias(e) => {
            // For aliases, we can just check transitively.
            let TypeRef::Ref(r) = &e.ty else {
                return Err(format!(
                    "\
                    failed to tag enum: inner type is a literal
                    - path = {}
                    - type = {:?}
                    ",
                    path, e.ty,
                ));
            };

            find_keyword(file, r.clone(), name)
        }
    }
}

fn make_keywords(file: &mut File, keywords: &BTreeMap<String, String>, errs: &mut Vec<String>) {
    for (path, by) in keywords {
        if let Err(err) = make_keyword(file, path, by) {
            errs.push(err);
        }
    }
}

fn make_keyword(file: &mut File, path: &str, value: &str) -> Result<(), String> {
    for ty in file.types.values_mut() {
        let TypeKind::Struct(s) = &mut ty.kind else {
            continue;
        };

        if let Some(field) = s.fields.get_mut(path) {
            field.ty = TypeRef::Keyword(value.into());
            return Ok(());
        }
    }

    Err(format!(
        "\
        can't make keyword: path not found:\n\
        - path = {path}\n\
        ",
    ))
}

// The two following functions (`common_prefix` and `common_suffix`) both work on words rather
// than regular characters. This is because we want to keep the original casing of the words
// (which are assumed to be in PascalCase).
//
// This means that if a common prefix extends over a word into the middle of it, the whole word
// will be conserved to avoid breaking it.

fn common_prefix<'a, I>(iter: I) -> &'a str
where
    I: Clone + Iterator<Item = &'a str>,
{
    let Some(reference) = iter.clone().next() else {
        return "";
    };

    // The length of the checked prefix so far.
    let mut prefix = 0;

    loop {
        let mut candidate = prefix;
        let mut prev: Option<char> = None;
        for c in reference[prefix..].chars() {
            // Look for a word boundary (transition from non-uppercase to uppercase for the case
            // of PascalCase).
            match (prev, c) {
                (Some(p), c) if !p.is_uppercase() && c.is_uppercase() => break,
                _ => (),
            }

            candidate += c.len_utf8();
            prev = Some(c);
        }

        // Check that all the other strings start with this word.
        for s in iter.clone() {
            if !s.starts_with(&reference[..candidate]) {
                // A string did not start with this word. We can stop here.
                return &reference[..prefix];
            }
        }

        // If the candidate is the whole string, we can stop here. It means that all
        // the strings are equal.
        if candidate == reference.len() {
            return reference;
        }

        // Otherwise, retry with the next word.
        // We found a new potential prefix.
        prefix = candidate;
    }
}

fn common_suffix<'a, I>(iter: I) -> &'a str
where
    I: Clone + Iterator<Item = &'a str>,
{
    let Some(reference) = iter.clone().next() else {
        return "";
    };

    // The length of the checked suffix so far.
    let mut suffix = 0;

    loop {
        let mut candidate = suffix;
        let mut prev: Option<char> = None;
        for c in reference[..reference.len() - suffix].chars().rev() {
            candidate += c.len_utf8();

            // Look for a word boundary (transition from non-uppercase to uppercase for the case
            // of PascalCase).
            match (c, prev) {
                (c, Some(p)) if c.is_uppercase() && !p.is_uppercase() => break,
                _ => (),
            }

            prev = Some(c);
        }

        // Check that all the other strings start with this word.
        for s in iter.clone() {
            if !s.ends_with(&reference[reference.len() - candidate..]) {
                // A string did not start with this word. We can stop here.
                return &reference[reference.len() - suffix..];
            }
        }

        // If the candidate is the whole string, we can stop here. It means that all
        // the strings are equal.
        if candidate == reference.len() {
            return reference;
        }

        // Otherwise, retry with the next word.
        // We found a new potential prefix.
        suffix = candidate;
    }
}
