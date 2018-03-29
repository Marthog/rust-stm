use std::sync::Arc;

/// `ArcList` is threadsafe, immutable linked list.
///
/// Operations like `push`, `prepend` and `pop` don't modify the shared part
/// of the list, but the beginning.
///
/// Cloning `ArcList` gives a reference to the same list.
///
///
/// `ArcList` provides convenience functions for patten matching
/// (`as_ref`, `split` and `into_splitted`),
/// for using the list in a functional way (`prepend`)
/// and for using the list like a stack (`push` and `pop`).
#[derive(Debug)]
pub struct ArcList<T> {
    head: Option<Arc<(T, ArcList<T>)>>,
}

// Cloning is always possible, even if T is not `Clone`.
impl<T> Clone for ArcList<T> {
    fn clone(&self) -> Self {
        ArcList { head: self.head.clone() }
    }
}

impl<T> ArcList<T> {
    #[inline]
    /// Create a new, empty list.
    pub fn new() -> Self {
        ArcList { head: None }
    }

    #[inline]
    /// Push an element to the beginning of the list.
    pub fn push(&mut self, t: T) {
        *self = ArcList::prepend(self.take(), t);
    }

    #[inline]
    /// Prepend a value to the existing list.
    pub fn prepend(self, t: T) -> Self {
        ArcList { head: Some(Arc::new((t, self))) }
    }

    #[inline]
    /// Check if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    #[inline]
    /// Split a list into head and tail. 
    /// `as_ref` Allows convenient pattern matching on the return value.
    pub fn as_ref(&self) -> Option<(&T, &ArcList<T>)> {
        self.head.as_ref().map(|h| (&h.0, &h.1))
    }

    #[inline]
    /// Split a list into head and tail. 
    /// `split` Allows convenient pattern matching on the return value.
    ///
    /// Unlike `as_ref` the tail is returned as a new list and
    /// not a reference.
    pub fn split(&self) -> Option<(&T, ArcList<T>)> {
        self.as_ref().map(|(x, xs)| (x, xs.clone()))
    }

    #[inline]
    /// Return the head of the list.
    pub fn head(&self) -> Option<&T> {
        self.head.as_ref().map(|h| &h.0)
    }

    #[inline]
    /// Take the inner of the list and leave `self` empty.
    pub fn take(&mut self) -> Self {
        ArcList { head: self.head.take() }
    }

    #[inline]
    /// Iterate over the elements of the list.
    pub fn iter<'a>(&'a self) -> IterRef<'a, T> {
        IterRef { list: self }
    }
}

impl<T: Clone> ArcList<T> {
    /// Split a list into head and tail. 
    /// `into_splitted` Allows convenient pattern matching on the return value.
    ///
    /// Unlike `split` `into_splitted` consumes self and returns the inner by value.
    ///
    /// Since `ArcList` is shared, we can not move the value out of list.
    /// Instead a fallback to `clone` is neccessary.
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

    /// Take an element from the beginning of the list.
    ///
    /// `push` and `pop` allow to use the list in a LIFO way.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        self.take().into_splitted().map(|(x, xs)| {
            *self = xs;
            x
        })
    }

    #[inline]
    /// Reverse the list.
    ///
    /// This function takes O(n), but may need to clone the inner values.
    pub fn reverse(mut self) -> Self {
        let mut new_list = ArcList::new();
        while let Some(t) = self.pop() {
            new_list.push(t)
        }
        new_list
    }

    #[inline]
    /// Iterate over the elements and consume them.
    ///
    /// The list is shared, therefore we need to clone the
    /// elements if neccessary.
    pub fn iter_cloned(self) -> IterClone<T> {
        IterClone { list: self }
    }
}

// Implement custom drop to avoid recursive calls and stack overflows.
impl<T> Drop for ArcList<T> {
    fn drop(&mut self) {
        while let Some(h) = self.head.take() {
            // try_unwrap returns the inner value if only one reference exists.
            // Then we drop the next element as well.
            // Otherwise, if there is more than one reference, the tail is shared
            // and does not need to be dropped.
            match Arc::try_unwrap(h) {
                Ok((_, tail)) => *self = tail,
                Err(_) => return
            }
        }
    }
}

/// Iterator over the items of an `ArcList` by reference.
pub struct IterRef<'a, T: 'a> {
    list: &'a ArcList<T>,
}

impl<'a, T> Iterator for IterRef<'a, T> {
    type Item = &'a T;

    #[inline]
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

/// Iterate over the elements and consume it.
///
/// The list is shared, therefore we need to clone the
/// elements if neccessary.
pub struct IterClone<T: Clone> {
    list: ArcList<T>,
}

impl<T: Clone> Iterator for IterClone<T> {
    type Item = T;

    #[inline]
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

