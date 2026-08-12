pub type MapMatrix<T> = Vec<Vec<T>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point<T>(pub T, pub T); // Point(x, y)

impl<T> Point<T> {
    pub fn new(x: T, y: T) -> Self {
        Self(x, y)
    }
}
