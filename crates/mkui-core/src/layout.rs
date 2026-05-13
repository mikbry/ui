//! Backend-agnostic layout primitives.
//!
//! These types describe the *intent* of a layout in terms shared between
//! every backend. A web backend maps them to flexbox CSS, a console backend
//! to row/column placement in a TUI grid, and a WGPU backend to its scene
//! layout pass. The translation rules live in each backend, not here.

/// Main-axis direction of a flex container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

/// Main-axis alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Cross-axis alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
    Baseline,
}

/// Edge-relative spacing in logical units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

impl Edges {
    pub fn all(value: u32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn symmetric(vertical: u32, horizontal: u32) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            left: horizontal,
            right: horizontal,
        }
    }
}

/// Full layout description for a container node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub direction: FlexDirection,
    pub justify: Justify,
    pub align: Align,
    pub gap: u32,
    pub padding: Edges,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Column,
            justify: Justify::Start,
            align: Align::Stretch,
            gap: 0,
            padding: Edges::default(),
        }
    }
}

impl Layout {
    pub fn row() -> Self {
        Self {
            direction: FlexDirection::Row,
            ..Self::default()
        }
    }

    pub fn column() -> Self {
        Self {
            direction: FlexDirection::Column,
            ..Self::default()
        }
    }

    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn gap(mut self, gap: u32) -> Self {
        self.gap = gap;
        self
    }

    pub fn padding(mut self, padding: Edges) -> Self {
        self.padding = padding;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_row_and_column_builders() {
        assert_eq!(Layout::row().direction, FlexDirection::Row);
        assert_eq!(Layout::column().direction, FlexDirection::Column);
    }

    #[test]
    fn layout_builder_methods_chain() {
        let layout = Layout::row()
            .justify(Justify::SpaceBetween)
            .align(Align::Center)
            .gap(8)
            .padding(Edges::all(4));

        assert_eq!(layout.direction, FlexDirection::Row);
        assert_eq!(layout.justify, Justify::SpaceBetween);
        assert_eq!(layout.align, Align::Center);
        assert_eq!(layout.gap, 8);
        assert_eq!(layout.padding, Edges::all(4));
    }

    #[test]
    fn edges_helpers() {
        assert_eq!(
            Edges::all(2),
            Edges {
                top: 2,
                right: 2,
                bottom: 2,
                left: 2
            }
        );
        assert_eq!(
            Edges::symmetric(1, 3),
            Edges {
                top: 1,
                right: 3,
                bottom: 1,
                left: 3
            }
        );
    }
}
