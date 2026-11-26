//! The Markdown page's document model, parser, find, links, selection and task boxes.
//! Applications own file loading and the window. The parser accepts text or bytes.

pub mod autolink;
pub mod emoji;
pub mod find;
pub mod frontmatter;
pub mod html;
pub mod limits;
pub mod links;
pub mod model;
pub mod parse;
pub mod rich;
pub mod select;
pub mod slug;
pub mod tasks;

pub use model::{Document, parse_bytes, parse_bytes_wiki, parse_bytes_wiki_embeds};
pub use parse::{parse, parse_wiki, parse_wiki_embeds};
