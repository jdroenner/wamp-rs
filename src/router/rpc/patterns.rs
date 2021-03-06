//! Contains the `RegistrationPatternNode` struct, which is used for constructing a trie corresponding
//! to pattern based registration
use super::super::{ConnectionInfo, random_id};
use ::{ID, URI, MatchingPolicy, InvocationPolicy};
use messages::Reason;
use std::sync::{Arc, Mutex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter, self};
use rand::thread_rng;
use rand::Rng;



/// Contains a trie corresponding to the registration patterns that connections have requested.
///
/// Each level of the trie corresponds to a fragment of a uri between the '.' character.
/// Thus each registration that starts with 'com' for example will be grouped together.
/// Registrations can be added and removed, and the connections that match a particular URI
/// can be iterated over using the `filter()` method.

pub struct RegistrationPatternNode<P:PatternData> {
    edges: HashMap<String, RegistrationPatternNode<P>>,
    connections: ProcdureCollection<P>,
    prefix_connections: ProcdureCollection<P>,
    id: ID,
    prefix_id: ID
}

/// Represents data that a pattern trie will hold
pub trait PatternData {
    fn get_id(&self) -> ID;
}

struct DataWrapper<P: PatternData> {
    registrant: P,
    policy: MatchingPolicy
}

struct ProcdureCollection<P: PatternData> {
    invocation_policy: InvocationPolicy,
    round_robin_counter: RefCell<usize>,
    procedures: Vec<DataWrapper<P>>
}

/// Represents an error caused during adding or removing patterns
#[derive(Debug)]
pub struct PatternError {
    reason: Reason
}


impl PatternError {
    #[inline]
    pub fn new(reason: Reason) -> PatternError {
        PatternError {
            reason: reason
        }
    }

    pub fn reason(self) -> Reason {
        self.reason
    }
}

impl PatternData for Arc<Mutex<ConnectionInfo>> {
    fn get_id(&self) -> ID {
        self.lock().unwrap().id
    }
}


impl<P:PatternData> Debug for RegistrationPatternNode <P>{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.fmt_with_indent(f, 0)
    }
}

impl<P:PatternData> ProcdureCollection<P> {
    fn add_procedure(&mut self, registrant: P, matching_policy: MatchingPolicy, invocation_policy: InvocationPolicy) -> Result<(), PatternError> {
        if self.procedures.len() == 0 || (invocation_policy == self.invocation_policy && invocation_policy != InvocationPolicy::Single) {
            self.procedures.push(DataWrapper {
                registrant: registrant,
                policy: matching_policy
            });
            self.invocation_policy = invocation_policy;
            Ok(())
        } else {
            Err(PatternError::new(Reason::ProcedureAlreadyExists))
        }
    }

    fn remove_procedure(&mut self, registrant_id: ID) {
        self.procedures.retain(|sub| sub.registrant.get_id() != registrant_id);
    }


    fn get_entry(&self) -> Option<&DataWrapper<P>> {

        match self.invocation_policy {
            InvocationPolicy::Single => self.procedures.first(),
            InvocationPolicy::First => self.procedures.first(),
            InvocationPolicy::Last => self.procedures.last(),
            InvocationPolicy::Random => {
                thread_rng().choose(&self.procedures)
            },
            InvocationPolicy::RoundRobin => {
                let mut counter = self.round_robin_counter.borrow_mut();
                if *counter >= self.procedures.len() {
                    *counter = 0
                }
                let result = self.procedures.get(*counter);
                *counter += 1;
                result
            }
        }

    }
}

impl<P:PatternData> RegistrationPatternNode<P> {

    fn fmt_with_indent(&self, f: &mut Formatter, indent: usize) -> fmt::Result {
        try!(writeln!(f, "{} pre: {:?} subs: {:?}",
            self.id,
            self.prefix_connections.procedures.iter().map(|sub| sub.registrant.get_id()).collect::<Vec<_>>(),
            self.connections.procedures.iter().map(|sub| sub.registrant.get_id()).collect::<Vec<_>>()));
        for (chunk, node) in self.edges.iter() {
            for _ in 0..indent * 2 {
                try!(write!(f, "  "));
            }
            try!(write!(f, "{} - ", chunk));
            try!(node.fmt_with_indent(f, indent + 1));
        }
        Ok(())
    }

    /// Add a new registration to the pattern trie with the given pattern and matching policy.
    pub fn register_with(&mut self, topic: &URI, registrant: P, matching_policy: MatchingPolicy, invocation_policy: InvocationPolicy) -> Result<ID, PatternError> {
        let mut uri_bits = topic.uri.split(".");
        let initial = match uri_bits.next() {
            Some(initial) => initial,
            None          => return Err(PatternError::new(Reason::InvalidURI))
        };
        let edge = self.edges.entry(initial.to_string()).or_insert(RegistrationPatternNode::new());
        edge.add_registration(uri_bits, registrant, matching_policy, invocation_policy)
    }

    /// Removes a registration from the pattern trie.
    pub fn unregister_with(&mut self, topic: &str, registrant: &P, is_prefix: bool) -> Result<ID, PatternError> {
        let uri_bits = topic.split(".");
        self.remove_registration(uri_bits, registrant.get_id(), is_prefix)
    }

    /// Gets a registrant that matches the given uri
    pub fn get_registrant_for(&self, procedure: URI) -> Result<(&P, ID, MatchingPolicy), PatternError> {
        let wrapper = self.find_registrant(&procedure.uri.split(".").collect(), 0);
        match wrapper {
            Some((ref data, id)) => {
                Ok((&data.registrant, id, data.policy))
            },
            None => {
                Err(PatternError::new(Reason::NoSuchProcedure))
            }
        }
    }

    /// Constructs a new RegistrationPatternNode to be used as the root of the trie
    #[inline]
    pub fn new() -> RegistrationPatternNode<P> {
        RegistrationPatternNode {
            edges: HashMap::new(),
            connections: ProcdureCollection {
                invocation_policy: InvocationPolicy::Single,
                round_robin_counter: RefCell::new(0),
                procedures: Vec::new()
            },
            prefix_connections:ProcdureCollection {
                invocation_policy: InvocationPolicy::Single,
                round_robin_counter: RefCell::new(0),
                procedures: Vec::new()
            },
            id: random_id(),
            prefix_id: random_id()
        }
    }

    fn add_registration<'a, I>(&mut self, mut uri_bits: I, registrant: P, matching_policy: MatchingPolicy, invocation_policy: InvocationPolicy) -> Result<ID, PatternError> where I: Iterator<Item=&'a str> {
        match uri_bits.next() {
            Some(uri_bit) => {
                if uri_bit.len() == 0 {
                    if matching_policy != MatchingPolicy::Wildcard {
                        return Err(PatternError::new(Reason::InvalidURI));
                    }
                }
                let edge = self.edges.entry(uri_bit.to_string()).or_insert(RegistrationPatternNode::new());
                edge.add_registration(uri_bits, registrant, matching_policy, invocation_policy)
            },
            None => {
                if matching_policy == MatchingPolicy::Prefix {
                    try!(self.prefix_connections.add_procedure(registrant, matching_policy, invocation_policy));
                    Ok(self.prefix_id)
                } else {
                    try!(self.connections.add_procedure(registrant, matching_policy, invocation_policy));
                    Ok(self.id)
                }
            }
        }
    }

    fn remove_registration<'a, I>(&mut self, mut uri_bits: I, registrant_id: u64, is_prefix: bool) -> Result<ID, PatternError> where I: Iterator<Item=&'a str> {
        // TODO consider deleting nodes in the tree if they are no longer in use.
        match uri_bits.next() {
            Some(uri_bit) => {
                if let Some(mut edge) = self.edges.get_mut(uri_bit) {
                    edge.remove_registration(uri_bits, registrant_id, is_prefix)
                } else {
                    return Err(PatternError::new(Reason::InvalidURI))
                }
            },
            None => {
                if is_prefix {
                    self.prefix_connections.remove_procedure(registrant_id);
                    Ok((self.prefix_id))
                } else {
                    self.connections.remove_procedure(registrant_id);
                    Ok((self.id))
                }
            }
        }
    }

    fn find_registrant(&self, uri_bits: &Vec<&str>, depth: usize) -> Option<(&DataWrapper<P>, ID)> {
        if depth == uri_bits.len() {
            if let Some(registrant) = self.connections.get_entry() {
                Some((registrant, self.id))
            } else if let Some(registrant) = self.prefix_connections.get_entry() {
                Some((registrant, self.prefix_id))
            } else {
                None
            }
        } else {
            if let Some((registrant, id)) = self.recurse(uri_bits, depth) {
                Some((registrant, id))
            } else if let Some(registrant) = self.prefix_connections.get_entry() {
                Some((registrant, self.prefix_id))
            } else {
                None
            }
        }
    }

    fn recurse(&self, uri_bits: &Vec<&str>, depth: usize) -> Option<(&DataWrapper<P>, ID)> {
        if let Some(edge) = self.edges.get(uri_bits[depth]) {
            if let Some(registrant) = edge.find_registrant(uri_bits, depth + 1) {
                return Some(registrant)
            }
        }
        if let Some(edge) = self.edges.get("") {
            if let Some(registrant) = edge.find_registrant(uri_bits, depth + 1) {
                return Some(registrant)
            }
        }
        None
    }

 }



 #[cfg(test)]
 mod test {
     use ::{URI, MatchingPolicy, InvocationPolicy, ID};
     use super::{RegistrationPatternNode, PatternData};

     #[derive(Clone)]
     struct MockData {
         id: ID
     }

     impl PatternData for MockData {
         fn get_id(&self) -> ID {
             self.id
         }
     }
     impl MockData {
         pub fn new(id: ID) -> MockData {
            MockData {
                id: id
            }
         }
     }

     #[test]
     fn adding_patterns() {
          let connection1 = MockData::new(1);
          let connection2 = MockData::new(2);
          let connection3 = MockData::new(3);
          let connection4 = MockData::new(4);
          let mut root = RegistrationPatternNode::new();

          let ids = [
            root.register_with(&URI::new("com.example.test..topic"), connection1, MatchingPolicy::Wildcard, InvocationPolicy::Single).unwrap(),
            root.register_with(&URI::new("com.example.test.specific.topic"), connection2, MatchingPolicy::Strict, InvocationPolicy::Single).unwrap(),
            root.register_with(&URI::new("com.example"), connection3, MatchingPolicy::Prefix, InvocationPolicy::Single).unwrap(),
            root.register_with(&URI::new("com.example.test"), connection4, MatchingPolicy::Prefix, InvocationPolicy::Single).unwrap(),
         ];
         println!("ids: {:?}", ids);

         assert_eq!(root.get_registrant_for(URI::new("com.example.test.specific.topic")).unwrap().1, ids[1]);
         assert_eq!(root.get_registrant_for(URI::new("com.example.test.another.topic")).unwrap().1, ids[0]);
         assert_eq!(root.get_registrant_for(URI::new("com.example.test.another")).unwrap().1, ids[3]);
         assert_eq!(root.get_registrant_for(URI::new("com.example")).unwrap().1, ids[2]);

     }

     #[test]
     fn removing_patterns() {
        let connection1 = MockData::new(1);
        let connection2 = MockData::new(2);
        let connection3 = MockData::new(3);
        let connection4 = MockData::new(4);
        let mut root = RegistrationPatternNode::new();

        let ids = [
          root.register_with(&URI::new("com.example.test..topic"), connection1.clone(), MatchingPolicy::Wildcard, InvocationPolicy::Single).unwrap(),
          root.register_with(&URI::new("com.example.test.specific.topic"), connection2, MatchingPolicy::Strict, InvocationPolicy::Single).unwrap(),
          root.register_with(&URI::new("com.example"), connection3, MatchingPolicy::Prefix, InvocationPolicy::Single).unwrap(),
          root.register_with(&URI::new("com.example.test"), connection4.clone(), MatchingPolicy::Prefix, InvocationPolicy::Single).unwrap(),
       ];

        root.unregister_with("com.example.test..topic", &connection1, false).unwrap();
        root.unregister_with("com.example.test", &connection4, true).unwrap();

        println!("ids: {:?}", ids);
        assert_eq!(root.get_registrant_for(URI::new("com.example.test.specific.topic")).unwrap().1, ids[1]);

     }
 }
