//! The Daitch-Mokotoff transformation table, transcribed from
//! The reference `dm_soundex`.
//!
//! # Shape, not meaning
//!
//! The reference stores the table as a nested the reference object that doubles as
//! a finite-state machine: a node's `'0'` key holds its code triple (marking it a
//! *legal* state) and every other key is a transition. Some nodes are plain
//! arrays instead of objects. That distinction is invisible to a reference
//! property access but very visible to a typed lookup, and it is **observable**:
//! `findRules` indexes nodes with characters straight from the input, so the
//! digit `0` collides with the legality marker and the digits `1` and `2` collide
//! with array indices. Reproducing the bug (`SoundExDM::process("B0") ==
//! "undefi"`) requires reproducing the shape.
//!
//! Each entry's second alternative — the genuine Daitch-Mokotoff dual code for
//! `CK`, `RS` and `RZ` — is stored but **never read**: The reference only ever
//! takes `legalState[0]`. Importing a correct D-M table would diverge.
//!
//! 121 legal states, maximum depth 7 (`SCHTSCH`). Generated from the reference
//! rather than typed by hand.

/// A code triple: `[start of word, before a vowel, any other situation]`.
/// `-1` means "emit nothing".
pub(crate) type Mapping = [i32; 3];

/// One node of the reference's `codes` object.
pub(crate) enum Node {
    /// An array-shaped node, `[[a,b,c]]` or `[[a,b,c],[d,e,f]]`. Indexing it
    /// with the digit `n` yields element `n`, which is how `CK1` walks into the
    /// unused dual code.
    Leaf(&'static [Mapping]),
    /// An object-shaped node. `mapping` is its `'0'` key, present only for legal
    /// states; `children` are its transitions, always uppercase ASCII letters.
    Branch {
        /// The `'0'` key, if this is a legal state.
        mapping: Option<Mapping>,
        /// Transitions, in source order (at most eight, so a linear scan beats
        /// any form of hashing).
        children: &'static [(u8, Node)],
    },
}

impl Node {
    /// `node[key]` for a letter or digit key.
    pub(crate) fn child(&self, key: u8) -> Option<&'static Node> {
        match self {
            // Arrays have no letter properties.
            Self::Leaf(_) => None,
            Self::Branch { children, .. } => children
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, node)| node),
        }
    }

    /// `node['<digit>']`, which reaches an array element or the `'0'` marker.
    pub(crate) fn indexed(&self, digit: usize) -> Option<Mapping> {
        match self {
            Self::Leaf(items) => items.get(digit).copied(),
            Self::Branch { mapping, .. } => (digit == 0).then_some(*mapping).flatten(),
        }
    }

    /// `node[0]` — the code triple, and the value the legality test reads.
    pub(crate) fn zero(&self) -> Option<Mapping> {
        match self {
            Self::Leaf(items) => items.first().copied(),
            Self::Branch { mapping, .. } => *mapping,
        }
    }
}

/// `codes[c]` for the first character of a token.
///
/// The reference's object has one key per letter, in alphabetical order, so the
/// lookup is a direct index rather than a scan — this runs once per code unit of
/// every word encoded. The ordering is checked by a test rather than assumed.
pub(crate) fn root(key: u8) -> Option<&'static Node> {
    if !key.is_ascii_uppercase() {
        return None;
    }
    let (letter, node) = &CODES[(key - b'A') as usize];
    debug_assert_eq!(*letter, key, "CODES must be in alphabetical order");
    Some(node)
}

/// The 26 first-letter entries of the Daitch-Mokotoff trie.
static CODES: &[(u8, Node)] = &[
    (
        b'A',
        Node::Branch {
            mapping: Some([0, -1, -1]),
            children: &[
                (b'I', Node::Leaf(&[[0, 1, -1]])),
                (b'J', Node::Leaf(&[[0, 1, -1]])),
                (b'Y', Node::Leaf(&[[0, 1, -1]])),
                (b'U', Node::Leaf(&[[0, 7, -1]])),
            ],
        },
    ),
    (b'B', Node::Leaf(&[[7, 7, 7]])),
    (
        b'C',
        Node::Branch {
            mapping: Some([5, 5, 5]),
            children: &[
                (
                    b'Z',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[(b'S', Node::Leaf(&[[4, 4, 4]]))],
                    },
                ),
                (
                    b'S',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[(b'Z', Node::Leaf(&[[4, 4, 4]]))],
                    },
                ),
                (b'K', Node::Leaf(&[[5, 5, 5], [45, 45, 45]])),
                (
                    b'H',
                    Node::Branch {
                        mapping: Some([5, 5, 5]),
                        children: &[(b'S', Node::Leaf(&[[5, 54, 54]]))],
                    },
                ),
            ],
        },
    ),
    (
        b'D',
        Node::Branch {
            mapping: Some([3, 3, 3]),
            children: &[
                (b'T', Node::Leaf(&[[3, 3, 3]])),
                (
                    b'Z',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[
                            (b'H', Node::Leaf(&[[4, 4, 4]])),
                            (b'S', Node::Leaf(&[[4, 4, 4]])),
                        ],
                    },
                ),
                (
                    b'S',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[
                            (b'H', Node::Leaf(&[[4, 4, 4]])),
                            (b'Z', Node::Leaf(&[[4, 4, 4]])),
                        ],
                    },
                ),
                (
                    b'R',
                    Node::Branch {
                        mapping: None,
                        children: &[
                            (b'S', Node::Leaf(&[[4, 4, 4]])),
                            (b'Z', Node::Leaf(&[[4, 4, 4]])),
                        ],
                    },
                ),
            ],
        },
    ),
    (
        b'E',
        Node::Branch {
            mapping: Some([0, -1, -1]),
            children: &[
                (b'I', Node::Leaf(&[[0, 1, -1]])),
                (b'J', Node::Leaf(&[[0, 1, -1]])),
                (b'Y', Node::Leaf(&[[0, 1, -1]])),
                (b'U', Node::Leaf(&[[1, 1, -1]])),
                (b'W', Node::Leaf(&[[1, 1, -1]])),
            ],
        },
    ),
    (
        b'F',
        Node::Branch {
            mapping: Some([7, 7, 7]),
            children: &[(b'B', Node::Leaf(&[[7, 7, 7]]))],
        },
    ),
    (b'G', Node::Leaf(&[[5, 5, 5]])),
    (b'H', Node::Leaf(&[[5, 5, -1]])),
    (
        b'I',
        Node::Branch {
            mapping: Some([0, -1, -1]),
            children: &[
                (b'A', Node::Leaf(&[[1, -1, -1]])),
                (b'E', Node::Leaf(&[[1, -1, -1]])),
                (b'O', Node::Leaf(&[[1, -1, -1]])),
                (b'U', Node::Leaf(&[[1, -1, -1]])),
            ],
        },
    ),
    (b'J', Node::Leaf(&[[4, 4, 4]])),
    (
        b'K',
        Node::Branch {
            mapping: Some([5, 5, 5]),
            children: &[
                (b'H', Node::Leaf(&[[5, 5, 5]])),
                (b'S', Node::Leaf(&[[5, 54, 54]])),
            ],
        },
    ),
    (b'L', Node::Leaf(&[[8, 8, 8]])),
    (
        b'M',
        Node::Branch {
            mapping: Some([6, 6, 6]),
            children: &[(b'N', Node::Leaf(&[[66, 66, 66]]))],
        },
    ),
    (
        b'N',
        Node::Branch {
            mapping: Some([6, 6, 6]),
            children: &[(b'M', Node::Leaf(&[[66, 66, 66]]))],
        },
    ),
    (
        b'O',
        Node::Branch {
            mapping: Some([0, -1, -1]),
            children: &[
                (b'I', Node::Leaf(&[[0, 1, -1]])),
                (b'J', Node::Leaf(&[[0, 1, -1]])),
                (b'Y', Node::Leaf(&[[0, 1, -1]])),
            ],
        },
    ),
    (
        b'P',
        Node::Branch {
            mapping: Some([7, 7, 7]),
            children: &[
                (b'F', Node::Leaf(&[[7, 7, 7]])),
                (b'H', Node::Leaf(&[[7, 7, 7]])),
            ],
        },
    ),
    (b'Q', Node::Leaf(&[[5, 5, 5]])),
    (
        b'R',
        Node::Branch {
            mapping: Some([9, 9, 9]),
            children: &[
                (b'Z', Node::Leaf(&[[94, 94, 94], [94, 94, 94]])),
                (b'S', Node::Leaf(&[[94, 94, 94], [94, 94, 94]])),
            ],
        },
    ),
    (
        b'S',
        Node::Branch {
            mapping: Some([4, 4, 4]),
            children: &[
                (
                    b'Z',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[
                            (b'T', Node::Leaf(&[[2, 43, 43]])),
                            (
                                b'C',
                                Node::Branch {
                                    mapping: None,
                                    children: &[
                                        (b'Z', Node::Leaf(&[[2, 4, 4]])),
                                        (b'S', Node::Leaf(&[[2, 4, 4]])),
                                    ],
                                },
                            ),
                            (b'D', Node::Leaf(&[[2, 43, 43]])),
                        ],
                    },
                ),
                (b'D', Node::Leaf(&[[2, 43, 43]])),
                (
                    b'T',
                    Node::Branch {
                        mapping: Some([2, 43, 43]),
                        children: &[
                            (
                                b'R',
                                Node::Branch {
                                    mapping: None,
                                    children: &[
                                        (b'Z', Node::Leaf(&[[2, 4, 4]])),
                                        (b'S', Node::Leaf(&[[2, 4, 4]])),
                                    ],
                                },
                            ),
                            (
                                b'C',
                                Node::Branch {
                                    mapping: None,
                                    children: &[(b'H', Node::Leaf(&[[2, 4, 4]]))],
                                },
                            ),
                            (
                                b'S',
                                Node::Branch {
                                    mapping: None,
                                    children: &[
                                        (b'H', Node::Leaf(&[[2, 4, 4]])),
                                        (
                                            b'C',
                                            Node::Branch {
                                                mapping: None,
                                                children: &[(b'H', Node::Leaf(&[[2, 4, 4]]))],
                                            },
                                        ),
                                    ],
                                },
                            ),
                        ],
                    },
                ),
                (
                    b'C',
                    Node::Branch {
                        mapping: Some([2, 4, 4]),
                        children: &[(
                            b'H',
                            Node::Branch {
                                mapping: Some([4, 4, 4]),
                                children: &[
                                    (
                                        b'T',
                                        Node::Branch {
                                            mapping: Some([2, 43, 43]),
                                            children: &[
                                                (
                                                    b'S',
                                                    Node::Branch {
                                                        mapping: None,
                                                        children: &[
                                                            (
                                                                b'C',
                                                                Node::Branch {
                                                                    mapping: None,
                                                                    children: &[(
                                                                        b'H',
                                                                        Node::Leaf(&[[2, 4, 4]]),
                                                                    )],
                                                                },
                                                            ),
                                                            (b'H', Node::Leaf(&[[2, 4, 4]])),
                                                        ],
                                                    },
                                                ),
                                                (
                                                    b'C',
                                                    Node::Branch {
                                                        mapping: None,
                                                        children: &[(
                                                            b'H',
                                                            Node::Leaf(&[[2, 4, 4]]),
                                                        )],
                                                    },
                                                ),
                                            ],
                                        },
                                    ),
                                    (b'D', Node::Leaf(&[[2, 43, 43]])),
                                ],
                            },
                        )],
                    },
                ),
                (
                    b'H',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[
                            (
                                b'T',
                                Node::Branch {
                                    mapping: Some([2, 43, 43]),
                                    children: &[
                                        (
                                            b'C',
                                            Node::Branch {
                                                mapping: None,
                                                children: &[(b'H', Node::Leaf(&[[2, 4, 4]]))],
                                            },
                                        ),
                                        (
                                            b'S',
                                            Node::Branch {
                                                mapping: None,
                                                children: &[(b'H', Node::Leaf(&[[2, 4, 4]]))],
                                            },
                                        ),
                                    ],
                                },
                            ),
                            (
                                b'C',
                                Node::Branch {
                                    mapping: None,
                                    children: &[(b'H', Node::Leaf(&[[2, 4, 4]]))],
                                },
                            ),
                            (b'D', Node::Leaf(&[[2, 43, 43]])),
                        ],
                    },
                ),
            ],
        },
    ),
    (
        b'T',
        Node::Branch {
            mapping: Some([3, 3, 3]),
            children: &[
                (
                    b'C',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[(b'H', Node::Leaf(&[[4, 4, 4]]))],
                    },
                ),
                (
                    b'Z',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[(b'S', Node::Leaf(&[[4, 4, 4]]))],
                    },
                ),
                (
                    b'S',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[
                            (b'Z', Node::Leaf(&[[4, 4, 4]])),
                            (b'H', Node::Leaf(&[[4, 4, 4]])),
                            (
                                b'C',
                                Node::Branch {
                                    mapping: None,
                                    children: &[(b'H', Node::Leaf(&[[4, 4, 4]]))],
                                },
                            ),
                        ],
                    },
                ),
                (
                    b'T',
                    Node::Branch {
                        mapping: None,
                        children: &[
                            (
                                b'S',
                                Node::Branch {
                                    mapping: Some([4, 4, 4]),
                                    children: &[
                                        (b'Z', Node::Leaf(&[[4, 4, 4]])),
                                        (
                                            b'C',
                                            Node::Branch {
                                                mapping: None,
                                                children: &[(b'H', Node::Leaf(&[[4, 4, 4]]))],
                                            },
                                        ),
                                    ],
                                },
                            ),
                            (
                                b'C',
                                Node::Branch {
                                    mapping: None,
                                    children: &[(b'H', Node::Leaf(&[[4, 4, 4]]))],
                                },
                            ),
                            (b'Z', Node::Leaf(&[[4, 4, 4]])),
                        ],
                    },
                ),
                (b'H', Node::Leaf(&[[3, 3, 3]])),
                (
                    b'R',
                    Node::Branch {
                        mapping: None,
                        children: &[
                            (b'Z', Node::Leaf(&[[4, 4, 4]])),
                            (b'S', Node::Leaf(&[[4, 4, 4]])),
                        ],
                    },
                ),
            ],
        },
    ),
    (
        b'U',
        Node::Branch {
            mapping: Some([0, -1, -1]),
            children: &[
                (b'E', Node::Leaf(&[[0, -1, -1]])),
                (b'I', Node::Leaf(&[[0, 1, -1]])),
                (b'J', Node::Leaf(&[[0, 1, -1]])),
                (b'Y', Node::Leaf(&[[0, 1, -1]])),
            ],
        },
    ),
    (b'V', Node::Leaf(&[[7, 7, 7]])),
    (b'W', Node::Leaf(&[[7, 7, 7]])),
    (b'X', Node::Leaf(&[[5, 54, 54]])),
    (b'Y', Node::Leaf(&[[1, -1, -1]])),
    (
        b'Z',
        Node::Branch {
            mapping: Some([4, 4, 4]),
            children: &[
                (
                    b'D',
                    Node::Branch {
                        mapping: Some([2, 43, 43]),
                        children: &[(
                            b'Z',
                            Node::Branch {
                                mapping: Some([2, 4, 4]),
                                children: &[(b'H', Node::Leaf(&[[2, 4, 4]]))],
                            },
                        )],
                    },
                ),
                (
                    b'H',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[(
                            b'D',
                            Node::Branch {
                                mapping: Some([2, 43, 43]),
                                children: &[(
                                    b'Z',
                                    Node::Branch {
                                        mapping: None,
                                        children: &[(b'H', Node::Leaf(&[[2, 4, 4]]))],
                                    },
                                )],
                            },
                        )],
                    },
                ),
                (
                    b'S',
                    Node::Branch {
                        mapping: Some([4, 4, 4]),
                        children: &[
                            (b'H', Node::Leaf(&[[4, 4, 4]])),
                            (
                                b'C',
                                Node::Branch {
                                    mapping: None,
                                    children: &[(b'H', Node::Leaf(&[[4, 4, 4]]))],
                                },
                            ),
                        ],
                    },
                ),
            ],
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_a_to_z_in_order() {
        assert_eq!(CODES.len(), 26);
        for (i, (letter, _)) in CODES.iter().enumerate() {
            assert_eq!(*letter, b'A' + i as u8);
        }
    }

    #[test]
    fn root_rejects_non_letters() {
        assert!(root(b'0').is_none());
        assert!(root(b'a').is_none()); // the encoder uppercases first
        assert!(root(b'A').is_some());
        assert!(root(b'Z').is_some());
    }

    #[test]
    fn the_table_has_121_legal_states() {
        fn count(node: &Node) -> usize {
            match node {
                Node::Leaf(_) => 1,
                Node::Branch { mapping, children } => {
                    usize::from(mapping.is_some())
                        + children.iter().map(|(_, n)| count(n)).sum::<usize>()
                }
            }
        }
        let total: usize = CODES.iter().map(|(_, n)| count(n)).sum();
        assert_eq!(total, 121, "transcribed from dm_soundex");
    }
}
