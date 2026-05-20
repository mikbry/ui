pub mod high_level;

pub use high_level::Mkui;

pub mod prelude {
    pub use crate::high_level::Mkui;
    pub use mkui_core::prelude::*;
}
