//! Helps in building messages conveniently

use crate::message;
use crate::wire::marshal_trait::Marshal;
use std::os::unix::io::RawFd;

#[derive(Default)]
pub struct MessageBuilder {
    msg: MarshalledMessage,
}

pub struct CallBuilder {
    msg: MarshalledMessage,
}
pub struct SignalBuilder {
    msg: MarshalledMessage,
}

impl MessageBuilder {
    /// New messagebuilder with the default little endian byteorder
    pub fn new() -> MessageBuilder {
        MessageBuilder {
            msg: MarshalledMessage::new(),
        }
    }

    /// New messagebuilder with a chosen byteorder
    pub fn with_byteorder(b: message::ByteOrder) -> MessageBuilder {
        MessageBuilder {
            msg: MarshalledMessage::with_byteorder(b),
        }
    }

    pub fn call(mut self, member: String) -> CallBuilder {
        self.msg.typ = message::MessageType::Call;
        self.msg.dynheader.member = Some(member);
        CallBuilder { msg: self.msg }
    }
    pub fn signal(mut self, interface: String, member: String, object: String) -> SignalBuilder {
        self.msg.typ = message::MessageType::Signal;
        self.msg.dynheader.member = Some(member);
        self.msg.dynheader.interface = Some(interface);
        self.msg.dynheader.object = Some(object);
        SignalBuilder { msg: self.msg }
    }
}

impl CallBuilder {
    pub fn on(mut self, object_path: String) -> Self {
        self.msg.dynheader.object = Some(object_path);
        self
    }

    pub fn with_interface(mut self, interface: String) -> Self {
        self.msg.dynheader.interface = Some(interface);
        self
    }

    pub fn at(mut self, destination: String) -> Self {
        self.msg.dynheader.destination = Some(destination);
        self
    }

    pub fn build(self) -> MarshalledMessage {
        self.msg
    }
}

impl SignalBuilder {
    pub fn to(mut self, destination: String) -> Self {
        self.msg.dynheader.destination = Some(destination);
        self
    }

    pub fn build(self) -> MarshalledMessage {
        self.msg
    }
}

#[derive(Debug)]
pub struct MarshalledMessage {
    pub body: MarshalledMessageBody,

    pub dynheader: message::DynamicHeader,

    // out of band data
    pub raw_fds: Vec<RawFd>,

    pub typ: message::MessageType,
    pub flags: u8,
}

impl Default for MarshalledMessage {
    fn default() -> Self {
        Self::new()
    }
}

/// This represents a message while it is being built before it is sent over the connection.
/// The body accepts everything that implements the Marshal trait (e.g. all basic types, strings, slices, Hashmaps,.....)
/// And you can of course write an Marshal impl for your own datastructures. See the doc on the Marshal trait what you have
/// to look out for when doing this though.
impl MarshalledMessage {
    pub fn get_buf(&self) -> &[u8] {
        &self.body.buf
    }
    pub fn get_sig(&self) -> &str {
        &self.body.sig
    }

    /// New message with the default little endian byteorder
    pub fn new() -> Self {
        MarshalledMessage {
            typ: message::MessageType::Invalid,
            dynheader: message::DynamicHeader::default(),

            raw_fds: Vec::new(),
            flags: 0,
            body: MarshalledMessageBody::new(),
        }
    }

    /// New messagebody with a chosen byteorder
    pub fn with_byteorder(b: message::ByteOrder) -> Self {
        MarshalledMessage {
            typ: message::MessageType::Invalid,
            dynheader: message::DynamicHeader::default(),

            raw_fds: Vec::new(),
            flags: 0,
            body: MarshalledMessageBody::with_byteorder(b),
        }
    }

    pub fn unmarshall_all<'a, 'e>(
        self,
    ) -> Result<message::Message<'a, 'e>, crate::wire::unmarshal::Error> {
        let params = if self.body.sig.is_empty() {
            vec![]
        } else {
            let sigs: Vec<_> = crate::signature::Type::parse_description(&self.body.sig)
                .map_err(|_| crate::wire::unmarshal::Error::InvalidSignature)?;

            let (_, params) = crate::wire::unmarshal::unmarshal_body(
                self.body.byteorder,
                &sigs,
                &self.body.buf,
                0,
            )?;
            params
        };
        Ok(message::Message {
            dynheader: self.dynheader,
            params,
            typ: self.typ,
            flags: self.flags,
            raw_fds: self.raw_fds,
        })
    }
}
/// The body accepts everything that implements the Marshal trait (e.g. all basic types, strings, slices, Hashmaps,.....)
/// And you can of course write an Marshal impl for your own datastrcutures
#[derive(Debug)]
pub struct MarshalledMessageBody {
    buf: Vec<u8>,
    sig: String,
    byteorder: message::ByteOrder,
}

impl Default for MarshalledMessageBody {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function you might need this if the dbus API you use has Variants somewhere inside nested structures. If the the
/// API has a Variant at the top-level you can use OutMessageBody::push_variant.
pub fn marshal_as_variant<P: Marshal>(
    p: P,
    byteorder: message::ByteOrder,
    buf: &mut Vec<u8>,
) -> Result<(), message::Error> {
    let mut sig_str = String::new();
    P::signature().to_str(&mut sig_str);
    crate::wire::util::pad_to_align(P::alignment(), buf);
    crate::wire::marshal_base::marshal_base_param(
        message::ByteOrder::LittleEndian,
        &crate::params::Base::Signature(sig_str),
        buf,
    )
    .unwrap();
    p.marshal(byteorder, buf)?;
    Ok(())
}

impl MarshalledMessageBody {
    /// New messagebody with the default little endian byteorder
    pub fn new() -> Self {
        MarshalledMessageBody {
            buf: Vec::new(),
            sig: String::new(),
            byteorder: message::ByteOrder::LittleEndian,
        }
    }

    /// New messagebody with a chosen byteorder
    pub fn with_byteorder(b: message::ByteOrder) -> Self {
        MarshalledMessageBody {
            buf: Vec::new(),
            sig: String::new(),
            byteorder: b,
        }
    }

    pub fn from_parts(buf: Vec<u8>, sig: String, byteorder: message::ByteOrder) -> Self {
        Self {
            buf,
            sig,
            byteorder,
        }
    }

    /// Clears the buffer and signature but holds on to the memory allocations. You can now start pushing new
    /// params as if this were a new message. This allows to reuse the OutMessage for the same dbus-message with different
    /// parameters without allocating the buffer every time.
    pub fn reset(&mut self) {
        self.sig.clear();
        self.buf.clear();
    }

    /// Push a Param with the old nested enum/struct approach. This is still supported for the case that in some corner cases
    /// the new trait/type based API does not work.
    pub fn push_old_param(&mut self, p: &crate::params::Param) -> Result<(), message::Error> {
        crate::wire::marshal_container::marshal_param(p, self.byteorder, &mut self.buf)?;
        p.sig().to_str(&mut self.sig);
        Ok(())
    }

    /// Convenience function to call push_old_param on a slice of Param
    pub fn push_old_params(&mut self, ps: &[crate::params::Param]) -> Result<(), message::Error> {
        for p in ps {
            self.push_old_param(p)?;
        }
        Ok(())
    }

    /// Append something that is Marshal to the message body
    pub fn push_param<P: Marshal>(&mut self, p: P) -> Result<(), message::Error> {
        p.marshal(self.byteorder, &mut self.buf)?;
        P::signature().to_str(&mut self.sig);
        Ok(())
    }

    /// Append two things that are Marshal to the message body
    pub fn push_param2<P1: Marshal, P2: Marshal>(
        &mut self,
        p1: P1,
        p2: P2,
    ) -> Result<(), message::Error> {
        self.push_param(p1)?;
        self.push_param(p2)?;
        Ok(())
    }

    /// Append three things that are Marshal to the message body
    pub fn push_param3<P1: Marshal, P2: Marshal, P3: Marshal>(
        &mut self,
        p1: P1,
        p2: P2,
        p3: P3,
    ) -> Result<(), message::Error> {
        self.push_param(p1)?;
        self.push_param(p2)?;
        self.push_param(p3)?;
        Ok(())
    }

    /// Append four things that are Marshal to the message body
    pub fn push_param4<P1: Marshal, P2: Marshal, P3: Marshal, P4: Marshal>(
        &mut self,
        p1: P1,
        p2: P2,
        p3: P3,
        p4: P4,
    ) -> Result<(), message::Error> {
        self.push_param(p1)?;
        self.push_param(p2)?;
        self.push_param(p3)?;
        self.push_param(p4)?;
        Ok(())
    }

    /// Append five things that are Marshal to the message body
    pub fn push_param5<P1: Marshal, P2: Marshal, P3: Marshal, P4: Marshal, P5: Marshal>(
        &mut self,
        p1: P1,
        p2: P2,
        p3: P3,
        p4: P4,
        p5: P5,
    ) -> Result<(), message::Error> {
        self.push_param(p1)?;
        self.push_param(p2)?;
        self.push_param(p3)?;
        self.push_param(p4)?;
        self.push_param(p5)?;
        Ok(())
    }

    /// Append any number of things that have the same type that is Marshal to the message body
    pub fn push_params<P: Marshal>(&mut self, params: &[P]) -> Result<(), message::Error> {
        for p in params {
            self.push_param(p)?;
        }
        Ok(())
    }

    /// Append something that is Marshal to the body but use a dbus Variant in the signature. This is necessary for some APIs
    pub fn push_variant<P: Marshal>(&mut self, p: P) -> Result<(), message::Error> {
        self.sig.push('v');
        marshal_as_variant(p, self.byteorder, &mut self.buf)
    }

    /// Create a parser to retrieve parameters from the body.
    #[inline]
    pub fn parser(&self) -> MessageBodyParser {
        MessageBodyParser::new(&self)
    }
}

#[test]
fn test_marshal_trait() {
    let mut body = MarshalledMessageBody::new();
    let bytes: &[&[_]] = &[&[4u64]];
    body.push_param(bytes).unwrap();

    assert_eq!(
        vec![12, 0, 0, 0, 8, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0],
        body.buf
    );
    assert_eq!(body.sig.as_str(), "aat");

    let mut body = MarshalledMessageBody::new();
    let mut map = std::collections::HashMap::new();
    map.insert("a", 4u32);

    body.push_param(&map).unwrap();
    assert_eq!(
        vec![12, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, b'a', 0, 0, 0, 4, 0, 0, 0,],
        body.buf
    );
    assert_eq!(body.sig.as_str(), "a{su}");

    let mut body = MarshalledMessageBody::new();
    body.push_param((11u64, "str", true)).unwrap();
    assert_eq!(body.sig.as_str(), "(tsb)");
    assert_eq!(
        vec![11, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, b's', b't', b'r', 0, 1, 0, 0, 0,],
        body.buf
    );

    struct MyStruct {
        x: u64,
        y: String,
    }

    use crate::wire::marshal_trait::Signature;
    impl Signature for &MyStruct {
        fn signature() -> crate::signature::Type {
            crate::signature::Type::Container(crate::signature::Container::Struct(vec![
                u64::signature(),
                String::signature(),
            ]))
        }

        fn alignment() -> usize {
            8
        }
    }
    impl Marshal for &MyStruct {
        fn marshal(
            &self,
            byteorder: message::ByteOrder,
            buf: &mut Vec<u8>,
        ) -> Result<(), message::Error> {
            // always align to 8
            crate::wire::util::pad_to_align(8, buf);
            self.x.marshal(byteorder, buf)?;
            self.y.marshal(byteorder, buf)?;
            Ok(())
        }
    }

    let mut body = MarshalledMessageBody::new();
    body.push_param(&MyStruct {
        x: 100,
        y: "A".to_owned(),
    })
    .unwrap();
    assert_eq!(body.sig.as_str(), "(ts)");
    assert_eq!(
        vec![100, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, b'A', 0,],
        body.buf
    );

    let mut body = MarshalledMessageBody::new();
    let emptymap: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut map = std::collections::HashMap::new();
    let mut map2 = std::collections::HashMap::new();
    map.insert("a", 4u32);
    map2.insert("a", &map);

    body.push_param(&map2).unwrap();
    body.push_param(&emptymap).unwrap();
    assert_eq!(body.sig.as_str(), "a{sa{su}}a{su}");
    assert_eq!(
        vec![
            28, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, b'a', 0, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0,
            0, b'a', 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0
        ],
        body.buf
    );

    // try to unmarshal stuff
    let mut body_iter = MessageBodyParser::new(&body);

    // first try some stuff that has the wrong signature
    type WrongNestedDict =
        std::collections::HashMap<String, std::collections::HashMap<String, u64>>;
    assert_eq!(
        body_iter.get::<WrongNestedDict>().err().unwrap(),
        crate::wire::unmarshal::Error::WrongSignature
    );
    type WrongStruct = (u64, i32, String);
    assert_eq!(
        body_iter.get::<WrongStruct>().err().unwrap(),
        crate::wire::unmarshal::Error::WrongSignature
    );

    // the get the correct type and make sure the content is correct
    type NestedDict = std::collections::HashMap<String, std::collections::HashMap<String, u32>>;
    let newmap2: NestedDict = body_iter.get().unwrap();
    assert_eq!(newmap2.len(), 1);
    assert_eq!(newmap2.get("a").unwrap().len(), 1);
    assert_eq!(*newmap2.get("a").unwrap().get("a").unwrap(), 4);

    // again try some stuff that has the wrong signature
    assert_eq!(
        body_iter.get::<WrongNestedDict>().err().unwrap(),
        crate::wire::unmarshal::Error::WrongSignature
    );
    assert_eq!(
        body_iter.get::<WrongStruct>().err().unwrap(),
        crate::wire::unmarshal::Error::WrongSignature
    );

    // get the empty map next
    let newemptymap: std::collections::HashMap<&str, u32> = body_iter.get().unwrap();
    assert_eq!(newemptymap.len(), 0);

    // test get2()
    let mut body_iter = body.parser();
    assert_eq!(
        body_iter.get2::<NestedDict, u16>().unwrap_err(),
        crate::wire::unmarshal::Error::WrongSignature
    );
    assert_eq!(
        body_iter
            .get3::<NestedDict, std::collections::HashMap<&str, u32>, u32>()
            .unwrap_err(),
        crate::wire::unmarshal::Error::EndOfMessage
    );

    // test to make sure body_iter is left unchanged from last failure and the map is
    // pulled out identically from above
    let (newmap2, newemptymap): (NestedDict, std::collections::HashMap<&str, u32>) =
        body_iter.get2().unwrap();
    // repeat assertions from above
    assert_eq!(newmap2.len(), 1);
    assert_eq!(newmap2.get("a").unwrap().len(), 1);
    assert_eq!(*newmap2.get("a").unwrap().get("a").unwrap(), 4);
    assert_eq!(newemptymap.len(), 0);
    assert_eq!(
        body_iter.get::<u16>().unwrap_err(),
        crate::wire::unmarshal::Error::EndOfMessage
    );
}

use crate::wire::unmarshal_trait::Unmarshal;
/// Iterate over the messages parameters
///
/// Because dbus allows for multiple toplevel params without an enclosing struct, this provides a simple Iterator (sadly not std::iterator::Iterator, since the types
/// of the parameters can be different)
/// that you can use to get the params one by one, calling `get::<T>` until you have obtained all the parameters.
/// If you try to get more parameters than the signature has types, it will return None, if you try to get a parameter that doesn not
/// fit the current one, it will return an Error::WrongSignature, but you can safely try other types, the iterator stays valid.
pub struct MessageBodyParser<'body> {
    buf_idx: usize,
    sig_idx: usize,
    sigs: Vec<crate::signature::Type>,
    body: &'body MarshalledMessageBody,
}

impl<'ret, 'body: 'ret> MessageBodyParser<'body> {
    pub fn new(body: &'body MarshalledMessageBody) -> Self {
        Self {
            buf_idx: 0,
            sig_idx: 0,
            sigs: crate::signature::Type::parse_description(&body.sig).unwrap(),
            body,
        }
    }

    pub fn get<T: Unmarshal<'ret, 'body>>(&mut self) -> Result<T, crate::wire::unmarshal::Error> {
        if self.sig_idx >= self.sigs.len() {
            return Err(crate::wire::unmarshal::Error::EndOfMessage);
        }
        if self.sigs[self.sig_idx] != T::signature() {
            return Err(crate::wire::unmarshal::Error::WrongSignature);
        }

        match T::unmarshal(self.body.byteorder, &self.body.buf, self.buf_idx) {
            Ok((bytes, res)) => {
                self.buf_idx += bytes;
                self.sig_idx += 1;
                Ok(res)
            }
            Err(e) => Err(e),
        }
    }
    /// Perform error handling for `get2(), get3()...` if `get_calls` fails.
    fn get_mult_helper<T, F>(
        &mut self,
        count: usize,
        get_calls: F,
    ) -> Result<T, crate::wire::unmarshal::Error>
    where
        F: FnOnce(&mut Self) -> Result<T, crate::wire::unmarshal::Error>,
    {
        if self.sig_idx + count > self.sigs.len() {
            return Err(crate::wire::unmarshal::Error::EndOfMessage);
        }
        let start_sig_idx = self.sig_idx;
        let start_buf_idx = self.buf_idx;
        match get_calls(self) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                self.sig_idx = start_sig_idx;
                self.buf_idx = start_buf_idx;
                Err(err)
            }
        }
    }
    pub fn get2<T1, T2>(&mut self) -> Result<(T1, T2), crate::wire::unmarshal::Error>
    where
        T1: Unmarshal<'ret, 'body>,
        T2: Unmarshal<'ret, 'body>,
    {
        let get_calls = |parser: &mut Self| {
            let ret1 = parser.get()?;
            let ret2 = parser.get()?;
            Ok((ret1, ret2))
        };
        self.get_mult_helper(2, get_calls)
    }
    pub fn get3<T1, T2, T3>(&mut self) -> Result<(T1, T2, T3), crate::wire::unmarshal::Error>
    where
        T1: Unmarshal<'ret, 'body>,
        T2: Unmarshal<'ret, 'body>,
        T3: Unmarshal<'ret, 'body>,
    {
        let get_calls = |parser: &mut Self| {
            let ret1 = parser.get()?;
            let ret2 = parser.get()?;
            let ret3 = parser.get()?;
            Ok((ret1, ret2, ret3))
        };
        self.get_mult_helper(3, get_calls)
    }
    pub fn get4<T1, T2, T3, T4>(
        &mut self,
    ) -> Result<(T1, T2, T3, T4), crate::wire::unmarshal::Error>
    where
        T1: Unmarshal<'ret, 'body>,
        T2: Unmarshal<'ret, 'body>,
        T3: Unmarshal<'ret, 'body>,
        T4: Unmarshal<'ret, 'body>,
    {
        let get_calls = |parser: &mut Self| {
            let ret1 = parser.get()?;
            let ret2 = parser.get()?;
            let ret3 = parser.get()?;
            let ret4 = parser.get()?;
            Ok((ret1, ret2, ret3, ret4))
        };
        self.get_mult_helper(4, get_calls)
    }
    pub fn get5<T1, T2, T3, T4, T5>(
        &mut self,
    ) -> Result<(T1, T2, T3, T4, T5), crate::wire::unmarshal::Error>
    where
        T1: Unmarshal<'ret, 'body>,
        T2: Unmarshal<'ret, 'body>,
        T3: Unmarshal<'ret, 'body>,
        T4: Unmarshal<'ret, 'body>,
        T5: Unmarshal<'ret, 'body>,
    {
        let get_calls = |parser: &mut Self| {
            let ret1 = parser.get()?;
            let ret2 = parser.get()?;
            let ret3 = parser.get()?;
            let ret4 = parser.get()?;
            let ret5 = parser.get()?;
            Ok((ret1, ret2, ret3, ret4, ret5))
        };
        self.get_mult_helper(5, get_calls)
    }
}
