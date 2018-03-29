use std::sync::Arc;

#[derive(Debug)]
pub struct ArcList<T> {
    head: Option<Arc<(T, ArcList<T>)>>,
}

impl<T> Clone for ArcList<T> {
    fn clone(&self) -> Self {
        ArcList { head: self.head.clone() }
    }
}

impl<T> ArcList<T> {
    /// Create a new, empty list.
    pub fn new() -> Self {
        ArcList { head: None }
    }

    pub fn push(&mut self, t: T) {
        *self = ArcList::prepend(self.take(), t);
    }

    /// Prepend a value to the existing list.
    pub fn prepend(self, t: T) -> Self {
        ArcList { head: Some(Arc::new((t, self))) }
    }

    /// Check if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Split the list into head and reference to the tail.
    pub fn as_ref(&self) -> Option<(&T, &ArcList<T>)> {
        self.head.as_ref().map(|h| (&h.0, &h.1))
    }

    /// Split the list into head and tail.
    ///
    /// Unlike `as_ref` the tail is returned as a new list and
    /// not a reference.
    pub fn split(&self) -> Option<(&T, ArcList<T>)> {
        self.as_ref().map(|(x, xs)| (x, xs.clone()))
    }

    /// Return the head of the list.
    pub fn head(&self) -> Option<&T> {
        self.head.as_ref().map(|h| &h.0)
    }

    /// Take the inner of the list and leave the original empty.
    pub fn take(&mut self) -> Self {
        ArcList { head: self.head.take() }
    }
}

impl<T: Clone> ArcList<T> {
    /// Split a list into head and tail.
    ///
    /// Unlike `split` `into_splitted` consumes self and returns the inner by value.
    ///
    /// Under normal condition this requires cloning the value, but the
    /// implementation minimizes the amount of neccessary clones.
    /// Especially when the reference is unique, neither the value, nor the `ArcList`
    /// are cloned.
    pub fn into_splitted(mut self) -> Option<(T, ArcList<T>)> {
        self.head.take().map(|h| match Arc::try_unwrap(h) {
            Ok(x) => x,
            Err(rf) => (*rf).clone(),
        })
    }

    pub fn pop(&mut self) -> Option<T> {
        self.take().into_splitted().map(|(x, xs)| {
            *self = xs;
            x
        })
    }

    /// Reverse the list.
    pub fn reverse(mut self) -> Self {
        let mut new_list = ArcList::new();
        while let Some(t) = self.pop() {
            new_list.push(t)
        }
        new_list
    }
}

impl<T> Drop for ArcList<T> {
    fn drop(&mut self) {
        while let Some(h) = self.head.take() {
            match Arc::try_unwrap(h) {
                Ok((_, tail)) => *self = tail,
                Err(_) => {
                    return;
                }
            }
        }
    }
}

pub struct IterRef<'a, T: 'a> {
    list: &'a ArcList<T>,
}

impl<'a, T> Iterator for IterRef<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        match self.list.as_ref() {
            Some((x, xs)) => {
                self.list = xs;
                Some(x)
            }
            None => None,
        }
    }
}

pub struct IterClone<T> {
    list: ArcList<T>,
}

impl<T: Clone> Iterator for IterClone<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.list.pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arclist_prepend() {
        let list = ArcList::new().prepend(1).prepend(2).prepend(3);

        assert_eq!(Some(&3), list.head());
    }

    /// Test if the destructor runs correctly.
    /// The naive implementation of linked lists creates stack overflows.
    #[test]
    fn test_long_list() {
        let mut list = ArcList::new();
        for i in 0..100000 {
            list = list.prepend(i);
        }
    }


    #[test]
    fn test_arclist_reverse() {
        let list = ArcList::new().prepend(1).prepend(2).prepend(3).reverse();

        assert_eq!(Some(&1), list.head());
    }
}

