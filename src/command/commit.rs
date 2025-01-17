use crate::object::commit::{Commit, Sign};
use crate::util;
use byteorder::{BigEndian, ByteOrder};
use hex;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs::File, io::Write};

#[derive(Clone, Debug)]
enum NodeType {
    Blob,
    Tree,
}

#[derive(Clone, Debug)]
struct Node {
    node_type: NodeType,
    mode: u32,
    name: String,
    hash: String,
    children: Vec<Node>,
}

pub fn commit(message: String) {
    let mut index_tree = Node {
        node_type: NodeType::Tree,
        mode: 0,
        name: "root".to_string(),
        hash: "".to_string(),
        children: Vec::new(),
    };
    decode_index_file(&mut index_tree);

    generate_tree_objects(&mut index_tree);
    let tree_hash = index_tree.hash.clone();
    let commit_hash = generate_commit_object(tree_hash, message);
    update_head(commit_hash);
}

fn travel_tree(node: &mut Node, path: &[&std::ffi::OsStr], mode: u32, hash: String) {
    if path.len() == 1 {
        let new_node = Node {
            node_type: NodeType::Blob,
            mode: mode,
            name: path[0].to_str().unwrap().to_string(),
            hash: hash,
            children: Vec::new(),
        };
        node.children.push(new_node);
        return;
    }

    if let Some((first, rest)) = path.split_first() {
        match node
            .children
            .iter_mut()
            .find(|child| child.name == first.to_str().unwrap())
        {
            Some(child_node) => {
                // childrenにディレクトリがある場合はそのまま移動
                travel_tree(child_node, rest, mode, hash);
            }
            None => {
                // ない場合は作成して追加して移動
                let new_node = Node {
                    node_type: NodeType::Tree,
                    mode: 0o040000,
                    name: first.to_str().unwrap().to_string(),
                    hash: "".to_string(),
                    children: Vec::new(),
                };
                node.children.push(new_node);
                let new_node = node.children.last_mut().unwrap();
                travel_tree(new_node, rest, mode, hash);
            }
        }
    }
}

fn construct_tree(index_tree: &mut Node, file_path: &str, mode: u32, hash: String) {
    let path = Path::new(file_path);
    let path_vec: Vec<_> = path.iter().collect();

    travel_tree(index_tree, &path_vec, mode, hash);
}

fn decode_index_entry(entry: &[u8], index_tree: &mut Node) -> usize {
    let mode = BigEndian::read_u32(&entry[24..28]);
    let hash = hex::encode(&entry[40..60]);
    let file_name_size = BigEndian::read_u16(&entry[60..62]);
    let file_path_end_byte = (62 + file_name_size) as usize;
    let file_path = std::str::from_utf8(&entry[62..file_path_end_byte]).unwrap();

    construct_tree(index_tree, file_path, mode, hash);

    let padding = 4 - (file_path_end_byte % 4);
    let next_byte = file_path_end_byte + padding;
    next_byte
}

fn decode_index_file(index_tree: &mut Node) {
    let mut file = File::open(".git/index").unwrap();
    let mut content = Vec::new();
    file.read_to_end(&mut content).unwrap();

    // entriesを上から1 entryずつ消費していく
    let entry_count = BigEndian::read_u32(&content[8..12]);
    let mut entries = &content[12..];
    for _ in 0..entry_count {
        let next_byte = decode_index_entry(&entries, index_tree);
        entries = &entries[next_byte..];
    }
}

fn generate_tree_object(node: &Node) -> String {
    // データの準備
    let mut contents: Vec<u8> = Vec::new();
    for child in &node.children {
        let mode_and_type = format!("{:0>6o} ", child.mode).as_bytes().to_vec();
        let pre_hash: Vec<u8> = child.hash[..].try_into().unwrap();
        let hash: Vec<u8> = hex::decode(pre_hash).unwrap();
        let path_name = format!("{}\0", child.name)[..].as_bytes().to_vec();

        contents.extend(mode_and_type.to_vec());
        contents.extend(path_name.to_vec());
        contents.extend(hash.to_vec());
    }

    let file_length = contents.len();
    let header = format!("tree {}\0", file_length);
    let header_vec: Vec<u8> = header.as_bytes().to_vec();

    let full_contents = [&header_vec[..], &contents[..]].concat();
    let hash = util::compress::hash(&full_contents);

    // ファイルの準備
    let file_directory = format!(".git/objects/{}", &hash[0..2]);
    let file_path = format!("{}/{}", file_directory, &hash[2..]);
    std::fs::create_dir_all(file_directory).unwrap();
    let mut file = File::create(file_path).unwrap();

    // zlib圧縮
    let compressed_contents = util::compress::zlib_compress(&full_contents);

    // ファイルに書き込み
    file.write_all(&compressed_contents).unwrap();

    hash
}

fn generate_tree_objects(index_tree: &mut Node) {
    // childrenを左から探索していく深さ優先探索
    for child in &mut index_tree.children {
        match child.node_type {
            NodeType::Blob => {
                // blobの場合は何もしない
            }
            NodeType::Tree => {
                // treeの場合は再帰的に呼び出す
                generate_tree_objects(child);
            }
        }
    }
    let hash = generate_tree_object(index_tree);
    index_tree.hash = hash;
}

fn generate_commit_object(tree_hash: String, message: String) -> String {
    let parent = util::path::get_head_commit_hash();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut commit = Commit {
        hash: "".to_string(),
        size: 0,
        tree: tree_hash,
        parents: match parent {
            Some(parent) => vec![parent],
            None => vec![],
        },
        author: Sign {
            name: "mehm8128".to_string(),
            email: "mehm8128@example.com".to_string(),
            time_stamp: now,
        },
        commiter: Sign {
            name: "mehm8128".to_string(),
            email: "mehm8128@example.com".to_string(),
            time_stamp: now,
        },
        message,
    };

    let mut content: Vec<u8> = Vec::new();
    content.extend(format!("tree {}\n", commit.tree).as_bytes());
    for parent in commit.parents {
        content.extend(format!("parent {}\n", parent).as_bytes());
    }
    content.extend(format!("author {}\n", commit.author.to_string()).as_bytes());
    content.extend(format!("committer {}\n", commit.commiter.to_string()).as_bytes());
    content.extend(format!("\n{}\n", commit.message).as_bytes());

    commit.size = content.len();
    let header = format!("commit {}\0", commit.size);
    let content = format!("{}{}", header, String::from_utf8(content).unwrap());
    let commit_hash = util::compress::hash(&content.as_bytes());
    commit.hash = commit_hash;

    let file_directory = format!(".git/objects/{}", &commit.hash[0..2]);
    let file_path = format!("{}/{}", file_directory, &commit.hash[2..]);
    match std::fs::create_dir(file_directory) {
        Ok(_) => {}
        Err(ref e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => panic!("{}", e),
    }
    let mut file = File::create(file_path).unwrap();

    // zlib圧縮
    let compressed_contents = util::compress::zlib_compress(content.as_bytes());
    file.write_all(&compressed_contents).unwrap();

    commit.hash
}

fn update_head(commit_hash: String) {
    let head_ref = util::path::get_head_ref();
    let mut file = File::create(head_ref).unwrap();
    file.write_all(commit_hash.as_bytes()).unwrap();
}
