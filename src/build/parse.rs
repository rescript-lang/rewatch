use super::build_types::*;
use super::logs;
use super::namespaces;
use super::packages;
use crate::bsconfig;
use crate::bsconfig::OneOrMore;
use crate::helpers;
use ahash::AHashSet;
use log::debug;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub fn generate_asts(
    build_state: &mut BuildState,
    inc: impl Fn() + std::marker::Sync,
) -> Result<String, String> {
    let mut has_failure = false;
    let mut stderr = "".to_string();

    build_state
        .modules
        .par_iter()
        .map(|(module_name, module)| {
            debug!("Generating AST for module: {}", module_name);
            let package = build_state
                .get_package(&module.package_name)
                .expect("Package not found");
            match &module.source_type {
                SourceType::MlMap(_mlmap) => {
                    let path = package.get_mlmap_path();
                    (module_name.to_owned(), Ok((path, None)), Ok(None), false)
                }

                SourceType::SourceFile(source_file) => {
                    let root_package = build_state.get_package(&build_state.root_config_name).unwrap();

                    let (ast_result, iast_result, dirty) = if source_file.implementation.parse_dirty
                        || source_file
                            .interface
                            .as_ref()
                            .map(|i| i.parse_dirty)
                            .unwrap_or(false)
                    {
                        inc();
                        let ast_result = generate_ast(
                            package.to_owned(),
                            root_package.to_owned(),
                            &source_file.implementation.path.to_owned(),
                            &build_state.rescript_version,
                            &build_state.bsc_path,
                            &build_state.workspace_root,
                        );

                        let iast_result = match source_file.interface.as_ref().map(|i| i.path.to_owned()) {
                            Some(interface_file_path) => generate_ast(
                                package.to_owned(),
                                root_package.to_owned(),
                                &interface_file_path.to_owned(),
                                &build_state.rescript_version,
                                &build_state.bsc_path,
                                &build_state.workspace_root,
                            )
                            .map(Some),
                            _ => Ok(None),
                        };

                        (ast_result, iast_result, true)
                    } else {
                        (
                            Ok((
                                helpers::get_basename(&source_file.implementation.path).to_string() + ".ast",
                                None,
                            )),
                            Ok(source_file
                                .interface
                                .as_ref()
                                .map(|i| (helpers::get_basename(&i.path).to_string() + ".iast", None))),
                            false,
                        )
                    };

                    (module_name.to_owned(), ast_result, iast_result, dirty)
                }
            }
        })
        .collect::<Vec<(
            String,
            Result<(String, Option<helpers::StdErr>), String>,
            Result<Option<(String, Option<helpers::StdErr>)>, String>,
            bool,
        )>>()
        .into_iter()
        .for_each(|(module_name, ast_result, iast_result, is_dirty)| {
            let result = if let Some(module) = build_state.modules.get_mut(&module_name) {
                // if the module is dirty, mark it also compile_dirty
                // do NOT set to false if the module is not parse_dirty, it needs to keep
                // the compile_dirty flag if it was set before
                if is_dirty {
                    // module.compile_dirty = true;
                    module.compile_dirty = true;
                }
                let package = build_state
                    .packages
                    .get(&module.package_name)
                    .expect("Package not found");

                if let SourceType::SourceFile(ref mut source_file) = module.source_type {
                    // We get Err(x) when there is a parse error. When it's Ok(_, Some(
                    // stderr_warnings )), the outputs are warnings
                    let ast_new_result = match ast_result {
                        // In case of a pinned (internal) dependency, we want to keep on
                        // propagating the warning with every compile. So we mark it as dirty for
                        // the next round
                        Ok((_path, Some(stderr_warnings))) if package.is_pinned_dep => {
                            source_file.implementation.parse_state = ParseState::Warning;
                            source_file.implementation.parse_dirty = true;
                            if let Some(interface) = source_file.interface.as_mut() {
                                interface.parse_dirty = false;
                            }
                            logs::append(package, &stderr_warnings);
                            stderr.push_str(&stderr_warnings);

                            // // After generating ASTs, handle embeds
                            // // Process embeds for the source file
                            // if let Err(err) = process_embeds(build_state, package, &module_name) {
                            //     has_failure = true;
                            //     stderr.push_str(&err);
                            // }
                            Ok(())
                        }
                        Ok((_path, Some(_))) | Ok((_path, None)) => {
                            // If we do have stderr_warnings here, the file is not a pinned
                            // dependency (so some external dep). We can ignore those
                            source_file.implementation.parse_state = ParseState::Success;
                            source_file.implementation.parse_dirty = false;
                            if let Some(interface) = source_file.interface.as_mut() {
                                interface.parse_dirty = false;
                            }
                            Ok(())
                        }
                        Err(err) => {
                            // Some compilation error
                            source_file.implementation.parse_state = ParseState::ParseError;
                            source_file.implementation.parse_dirty = true;
                            logs::append(package, &err);
                            has_failure = true;
                            stderr.push_str(&err);
                            Err(())
                        }
                    };

                    // We get Err(x) when there is a parse error. When it's Ok(_, Some(( _path,
                    // stderr_warnings ))), the outputs are warnings
                    let iast_new_result = match iast_result {
                        // In case of a pinned (internal) dependency, we want to keep on
                        // propagating the warning with every compile. So we mark it as dirty for
                        // the next round
                        Ok(Some((_path, Some(stderr_warnings)))) if package.is_pinned_dep => {
                            if let Some(interface) = source_file.interface.as_mut() {
                                interface.parse_state = ParseState::Warning;
                                interface.parse_dirty = true;
                            }
                            logs::append(package, &stderr_warnings);
                            stderr.push_str(&stderr_warnings);
                            Ok(())
                        }
                        Ok(Some((_, None))) | Ok(Some((_, Some(_)))) => {
                            // If we do have stderr_warnings here, the file is not a pinned
                            // dependency (so some external dep). We can ignore those
                            if let Some(interface) = source_file.interface.as_mut() {
                                interface.parse_state = ParseState::Success;
                                interface.parse_dirty = false;
                            }
                            Ok(())
                        }
                        Err(err) => {
                            // Some compilation error
                            if let Some(interface) = source_file.interface.as_mut() {
                                interface.parse_state = ParseState::ParseError;
                                interface.parse_dirty = true;
                            }
                            logs::append(package, &err);
                            has_failure = true;
                            stderr.push_str(&err);
                            Err(())
                        }
                        Ok(None) => {
                            // The file had no interface file associated
                            Ok(())
                        }
                    };
                    match (ast_new_result, iast_new_result) {
                        (Ok(()), Ok(())) => Ok(()),
                        _ => Err(()),
                    }
                } else {
                    Err(())
                }
            } else {
                Err(())
            };
            match result {
                Ok(()) => {
                    if let Err(err) = process_embeds(build_state, &module_name) {
                        has_failure = true;
                        stderr.push_str(&err);
                    }
                }
                Err(()) => (),
            }
        });

    // compile the mlmaps of dirty modules
    // first collect dirty packages
    let dirty_packages = build_state
        .modules
        .iter()
        .filter(|(_, module)| module.compile_dirty)
        .map(|(_, module)| module.package_name.clone())
        .collect::<AHashSet<String>>();

    build_state.modules.iter_mut().for_each(|(module_name, module)| {
        let is_dirty = match &module.source_type {
            SourceType::MlMap(_) => {
                if dirty_packages.contains(&module.package_name) {
                    let package = build_state
                        .packages
                        .get(&module.package_name)
                        .expect("Package not found");
                    // probably better to do this in a different function
                    // specific to compiling mlmaps
                    let compile_path = package.get_mlmap_compile_path();
                    let mlmap_hash = helpers::compute_file_hash(&compile_path);
                    namespaces::compile_mlmap(package, module_name, &build_state.bsc_path);
                    let mlmap_hash_after = helpers::compute_file_hash(&compile_path);

                    let suffix = package
                        .namespace
                        .to_suffix()
                        .expect("namespace should be set for mlmap module");
                    // copy the mlmap to the bs build path for editor tooling
                    let base_build_path = package.get_build_path() + "/" + &suffix;
                    let base_bs_build_path = package.get_bs_build_path() + "/" + &suffix;
                    let _ = std::fs::copy(
                        base_build_path.to_string() + ".cmi",
                        base_bs_build_path.to_string() + ".cmi",
                    );
                    let _ = std::fs::copy(
                        base_build_path.to_string() + ".cmt",
                        base_bs_build_path.to_string() + ".cmt",
                    );
                    let _ = std::fs::copy(
                        base_build_path.to_string() + ".cmj",
                        base_bs_build_path.to_string() + ".cmj",
                    );
                    let _ = std::fs::copy(
                        base_build_path.to_string() + ".mlmap",
                        base_bs_build_path.to_string() + ".mlmap",
                    );
                    match (mlmap_hash, mlmap_hash_after) {
                        (Some(digest), Some(digest_after)) => !digest.eq(&digest_after),
                        _ => true,
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if is_dirty {
            module.compile_dirty = is_dirty;
        }
    });

    if has_failure {
        Err(stderr)
    } else {
        Ok(stderr)
    }
}

pub fn parser_args(
    config: &bsconfig::Config,
    root_config: &bsconfig::Config,
    filename: &str,
    version: &str,
    workspace_root: &Option<String>,
    root_path: &str,
    contents: &str,
) -> (String, Vec<String>) {
    let file = &filename.to_string();
    let path = PathBuf::from(filename);
    let ast_extension = path_to_ast_extension(&path);
    let ast_path = (helpers::get_basename(&file.to_string()).to_owned()) + ast_extension;
    let ppx_flags = bsconfig::flatten_ppx_flags(
        &if let Some(workspace_root) = workspace_root {
            format!("{}/node_modules", &workspace_root)
        } else {
            format!("{}/node_modules", &root_path)
        },
        &filter_ppx_flags(&config.ppx_flags, contents),
        &config.name,
    );
    let jsx_args = root_config.get_jsx_args();
    let jsx_module_args = root_config.get_jsx_module_args();
    let jsx_mode_args = root_config.get_jsx_mode_args();
    let uncurried_args = root_config.get_uncurried_args(version);
    let bsc_flags = bsconfig::flatten_flags(&config.bsc_flags);
    let embed_flags = bsconfig::get_embed_generators_bsc_flags(&config);

    let file = "../../".to_string() + file;
    (
        ast_path.to_string(),
        vec![
            vec!["-bs-v".to_string(), format!("{}", version)],
            ppx_flags,
            jsx_args,
            jsx_module_args,
            jsx_mode_args,
            uncurried_args,
            bsc_flags,
            vec![
                "-absname".to_string(),
                "-bs-ast".to_string(),
                "-o".to_string(),
                ast_path.to_string(),
                file,
            ],
            embed_flags,
        ]
        .concat(),
    )
}

fn generate_ast(
    package: packages::Package,
    root_package: packages::Package,
    filename: &str,
    version: &str,
    bsc_path: &str,
    workspace_root: &Option<String>,
) -> Result<(String, Option<helpers::StdErr>), String> {
    let file_path = PathBuf::from(&package.path).join(filename);
    let contents = helpers::read_file(&file_path).expect("Error reading file");

    let build_path_abs = package.get_build_path();
    let (ast_path, parser_args) = parser_args(
        &package.bsconfig,
        &root_package.bsconfig,
        filename,
        version,
        workspace_root,
        &root_package.path,
        &contents,
    );

    /* Create .ast */
    let result = if let Some(res_to_ast) = Some(
        Command::new(bsc_path)
            .current_dir(&build_path_abs)
            .args(parser_args)
            .output()
            .expect("Error converting .res to .ast"),
    ) {
        let stderr = std::str::from_utf8(&res_to_ast.stderr).expect("Expect StdErr to be non-null");
        if helpers::contains_ascii_characters(stderr) {
            if res_to_ast.status.success() {
                Ok((ast_path, Some(stderr.to_string())))
            } else {
                Err(format!("Error in {}:\n{}", package.name, stderr))
            }
        } else {
            Ok((ast_path, None))
        }
    } else {
        println!("Parsing file {}...", filename);
        Err(format!(
            "Could not find canonicalize_string_path for file {} in package {}",
            filename, package.name
        ))
    };
    match &result {
        Ok((ast_path, _)) => {
            let dir = std::path::Path::new(filename).parent().unwrap();
            let _ = std::fs::copy(
                build_path_abs.to_string() + "/" + ast_path,
                std::path::Path::new(&package.get_bs_build_path())
                    .join(dir)
                    .join(ast_path),
            );
        }
        Err(_) => (),
    }
    result
}

fn path_to_ast_extension(path: &Path) -> &str {
    let extension = path.extension().unwrap().to_str().unwrap();
    if helpers::is_interface_ast_file(extension) {
        ".iast"
    } else {
        ".ast"
    }
}

// Function to process embeds
fn process_embeds(build_state: &mut BuildState, module_name: &str) -> Result<(), String> {
    let module = build_state.modules.get(module_name).unwrap();
    let package = build_state.packages.get(&module.package_name).unwrap();
    let source_file = match &module.source_type {
        SourceType::SourceFile(source_file) => source_file,
        _ => panic!("Module {} is not a source file", module_name),
    };

    let ast_path_str = package.get_ast_path(&source_file.implementation.path);
    let ast_path = Path::new(&ast_path_str);
    let embeds_json_path = ast_path.with_extension("embeds.json");

    // Read and parse the embeds JSON file
    if embeds_json_path.exists() {
        let embeds_json = helpers::read_file(&embeds_json_path).map_err(|e| e.to_string())?;
        let embeds_data: Vec<EmbedJsonData> =
            serde_json::from_str(&embeds_json).map_err(|e| e.to_string())?;

        // Process each embed
        let embeds = embeds_data
            .into_iter()
            .map(|embed_data| {
                let embed_path = package.generated_file_folder.join(&embed_data.filename);
                let hash = helpers::compute_string_hash(&embed_data.contents);
                let dirty = is_embed_dirty(&embed_path, &hash.to_string());
                // embed_path is the path of the generated rescript file, let's add this path to the build state
                // Add the embed_path as a rescript source file to the build state
                let relative_path = Path::new(&embed_path).to_string_lossy();
                let module_name = helpers::file_path_to_module_name(&relative_path, &package.namespace);
                let last_modified = std::fs::metadata(&embed_path)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::now());

                if dirty {
                    // run the embed file
                    // Find the embed generator based on the tag
                    if let Some(embed_generator) =
                        package.bsconfig.embed_generators.as_ref().and_then(|generators| {
                            generators.iter().find(|gen| gen.tags.contains(&embed_data.tag))
                        })
                    {
                        // TODO(embed) Needs to be relative to package root? Join with package path?
                        let mut command = Command::new(&embed_generator.path);

                        // Run the embed generator
                        let output = command
                            .stdin(std::process::Stdio::piped())
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .spawn()
                            .and_then(|mut child| {
                                use std::io::Write;
                                let contents = format!("{}\n{}", embed_data.tag, embed_data.contents);
                                child.stdin.as_mut().unwrap().write_all(contents.as_bytes())?;
                                child.wait_with_output()
                            })
                            .map_err(|e| format!("Failed to run embed generator: {}", e))?;

                        if !output.status.success() {
                            return Err(format!(
                                "Embed generator failed: {}",
                                String::from_utf8_lossy(&output.stderr)
                            ));
                        }

                        let generated_content = String::from_utf8_lossy(&output.stdout).into_owned();
                        let generated_file_contents = format!("// HASH: {}\n{}", hash, generated_content);

                        // Write the output to the embed file
                        std::fs::write(&embed_path, generated_file_contents)
                            .map_err(|e| format!("Failed to write embed file: {}", e))?;
                    } else {
                        return Err(format!("No embed generator found for tag: {}", embed_data.tag));
                    }
                }
                if !build_state.modules.contains_key(&module_name) {
                    let implementation = Implementation {
                        path: relative_path.to_string(),
                        parse_state: ParseState::Pending,
                        compile_state: CompileState::Pending,
                        last_modified,
                        parse_dirty: true,
                    };

                    let source_file = SourceFile {
                        implementation,
                        interface: None,
                        embeds: Vec::new(),
                    };

                    let module = Module {
                        source_type: SourceType::SourceFile(source_file),
                        deps: AHashSet::new(),
                        dependents: AHashSet::new(),
                        package_name: package.name.clone(),
                        compile_dirty: true,
                        last_compiled_cmi: None,
                        last_compiled_cmt: None,
                    };

                    build_state.modules.insert(module_name.to_string(), module);
                    build_state.module_names.insert(module_name.to_string());
                } else if dirty {
                    if let Some(module) = build_state.modules.get_mut(&module_name) {
                        if let SourceType::SourceFile(source_file) = &mut module.source_type {
                            source_file.implementation.parse_dirty = true;
                        }
                    }
                }

                Ok(Embed {
                    hash: hash.to_string(),
                    embed: embed_data,
                    dirty,
                })
            })
            .collect::<Vec<Result<Embed, String>>>();

        let module = build_state.modules.get_mut(module_name).unwrap();
        match module.source_type {
            SourceType::SourceFile(ref mut source_file) => {
                source_file.embeds = embeds.into_iter().filter_map(|result| result.ok()).collect();
            }
            _ => (),
        };
    }

    Ok(())
}

fn is_embed_dirty(embed_path: &Path, hash: &str) -> bool {
    // Check if the embed file exists and compare hashes
    // the first line of the generated rescript file is a comment with the following format:
    // "// HASH: <hash>"
    // if the hash is different from the hash in the embed_data, the embed is dirty
    // if the file does not exist, the embed is dirty
    // if the file exists but the hash is not present, the embed is dirty
    // if the file exists but the hash is present but different from the hash in the embed_data, the embed is dirty
    // if the file exists but the hash is present and the same as the hash in the embed_data, the embed is not dirty
    if !embed_path.exists() {
        return true;
    }

    let first_line = match helpers::read_file(embed_path) {
        Ok(contents) => contents.lines().next().unwrap_or("").to_string(),
        Err(_) => return true,
    };

    if !first_line.starts_with("// HASH: ") {
        return true;
    }

    let file_hash = first_line.trim_start_matches("// HASH: ");
    file_hash != hash
}

fn include_ppx(flag: &str, contents: &str) -> bool {
    if flag.contains("bisect") {
        return std::env::var("BISECT_ENABLE").is_ok();
    } else if (flag.contains("graphql-ppx") || flag.contains("graphql_ppx")) && !contents.contains("%graphql")
    {
        return false;
    } else if flag.contains("spice") && !contents.contains("@spice") {
        return false;
    } else if flag.contains("rescript-relay") && !contents.contains("%relay") {
        return false;
    } else if flag.contains("re-formality") && !contents.contains("%form") {
        return false;
    }
    return true;
}

fn filter_ppx_flags(
    ppx_flags: &Option<Vec<OneOrMore<String>>>,
    contents: &str,
) -> Option<Vec<OneOrMore<String>>> {
    // get the environment variable "BISECT_ENABLE" if it exists set the filter to "bisect"
    ppx_flags.as_ref().map(|flags| {
        flags
            .iter()
            .filter(|flag| match flag {
                bsconfig::OneOrMore::Single(str) => include_ppx(str, contents),
                bsconfig::OneOrMore::Multiple(str) => include_ppx(str.first().unwrap(), contents),
            })
            .map(|x| x.to_owned())
            .collect::<Vec<OneOrMore<String>>>()
    })
}
