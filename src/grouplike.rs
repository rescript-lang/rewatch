pub trait Semigroup {
    fn mappend(&self, b: Self) -> Self;
}

pub trait Monoid: Semigroup {
    fn mempty() -> Self;
}

impl Semigroup for String {
    fn mappend(&self, b: String) -> String {
        (self.to_owned() + &b.to_owned()).to_owned()
    }
}

impl Monoid for String {
    fn mempty() -> String {
        "".to_owned()
    }
}

impl<T: Semigroup + Clone> Semigroup for std::option::Option<T> {
    fn mappend(&self, b: Option<T>) -> Option<T> {
        match (self, b) {
            (Some(lhs), None) => Some(lhs.to_owned()),
            (None, Some(rhs)) => Some(rhs.to_owned()),
            (Some(lhs), Some(rhs)) => Some(lhs.to_owned().mappend(rhs.to_owned())),
            (None, None) => None,
        }
    }
}
