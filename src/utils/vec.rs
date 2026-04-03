pub trait VecExt<T> {
    fn contains_any(&self, other: &[T]) -> bool
    where
        T: PartialEq;
}

impl<T> VecExt<T> for Vec<T> {
    fn contains_any(&self, other: &[T]) -> bool
    where
        T: PartialEq,
    {
        other.iter().any(|item| self.contains(item))
    }
}
