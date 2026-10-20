//! A class-file reader (JVMS chapter 4), std only: what a class is, what it
//! declares, and what it uses of other classes.
//!
//! Only what the analysis needs is kept: the class, its superclass and
//! interfaces, its fields and methods (name and descriptor), the members of
//! other classes it refers to (`Fieldref`, `Methodref`,
//! `InterfaceMethodref`), the classes it names, and annotations (where
//! Mixin's `@Mixin` and `@Inject` live). Code bodies are not decoded: the
//! constant pool already holds every symbolic reference the code can make.
//!
//! Hostile input is an error, never a panic: every read is bounds-checked,
//! and nested annotation values stop at a fixed depth.

/// Annotations nest (an annotation in an array in an annotation...): this deep
/// is far beyond any real class.
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefKind {
    Field,
    Method,
    InterfaceMethod,
}

/// A member of another class, as the constant pool names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemberRef {
    pub kind: RefKind,
    /// Internal name (`net/minecraft/world/level/Level`).
    pub owner: String,
    pub name: String,
    pub descriptor: String,
}

/// An annotation element value; primitives are not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    /// An `int`, `short`, `byte`, `char` or `boolean` value (`require = 1`).
    Int(i64),
    /// A class literal, as a descriptor (`Lnet/minecraft/X;`).
    Class(String),
    Enum {
        descriptor: String,
        name: String,
    },
    Annotation(Annotation),
    Array(Vec<Value>),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// `Lorg/spongepowered/asm/mixin/Mixin;`
    pub descriptor: String,
    pub elements: Vec<(String, Value)>,
}

impl Annotation {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.elements
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub descriptor: String,
    pub access: u16,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassFile {
    pub major: u16,
    pub access: u16,
    pub name: String,
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    pub fields: Vec<Member>,
    pub methods: Vec<Member>,
    /// Every member of another (or this) class the constant pool names.
    pub refs: Vec<MemberRef>,
    /// Every class the constant pool names (array classes as their element).
    pub classes: Vec<String>,
    /// Class annotations, visible and invisible (Mixin's are invisible).
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
enum Const {
    Utf8(String),
    Class(u16),
    NameAndType(u16, u16),
    Ref(RefKind, u16, u16),
    /// CONSTANT_Integer: what an `int` or `boolean` annotation value points to.
    Int(i32),
    /// Integer, Float, String, MethodHandle, ... and the second slot of a
    /// Long or Double: nothing the analysis reads.
    Other,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> R<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or("length overflow")?;
        let s = self.b.get(self.at..end).ok_or("truncated class file")?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> R<u16> {
        let s = self.bytes(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> R<u32> {
        let s = self.bytes(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
}

struct Pool(Vec<Const>);

impl Pool {
    fn get(&self, i: u16) -> R<&Const> {
        self.0
            .get(usize::from(i))
            .ok_or_else(|| format!("constant #{i} out of range"))
    }
    fn utf8(&self, i: u16) -> R<&str> {
        match self.get(i)? {
            Const::Utf8(s) => Ok(s),
            _ => Err(format!("constant #{i} is not text")),
        }
    }
    fn class(&self, i: u16) -> R<&str> {
        match self.get(i)? {
            Const::Class(n) => self.utf8(*n),
            _ => Err(format!("constant #{i} is not a class")),
        }
    }
}

/// `[[Lnet/minecraft/X;` -> `net/minecraft/X`; a primitive array -> None.
pub fn element_class(name: &str) -> Option<&str> {
    let e = name.trim_start_matches('[');
    if e.len() == name.len() {
        return Some(name);
    }
    e.strip_prefix('L')?.strip_suffix(';')
}

pub fn parse(bytes: &[u8]) -> R<ClassFile> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.u32()? != 0xCAFE_BABE {
        return Err("not a class file".into());
    }
    let _minor = r.u16()?;
    let major = r.u16()?;

    let count = r.u16()?;
    let mut pool = vec![Const::Other];
    while pool.len() < usize::from(count) {
        let tag = r.u8()?;
        let c = match tag {
            1 => {
                let n = r.u16()?;
                Const::Utf8(String::from_utf8_lossy(r.bytes(usize::from(n))?).into_owned())
            }
            7 => Const::Class(r.u16()?),
            9..=11 => {
                let kind = match tag {
                    9 => RefKind::Field,
                    10 => RefKind::Method,
                    _ => RefKind::InterfaceMethod,
                };
                Const::Ref(kind, r.u16()?, r.u16()?)
            }
            12 => Const::NameAndType(r.u16()?, r.u16()?),
            3 => Const::Int(r.u32()? as i32),
            4 => {
                r.bytes(4)?;
                Const::Other
            }
            5 | 6 => {
                r.bytes(8)?;
                pool.push(Const::Other);
                Const::Other
            }
            8 | 16 | 19 | 20 => {
                r.bytes(2)?;
                Const::Other
            }
            15 => {
                r.bytes(3)?;
                Const::Other
            }
            17 | 18 => {
                r.bytes(4)?;
                Const::Other
            }
            t => return Err(format!("unknown constant tag {t}")),
        };
        pool.push(c);
    }
    let pool = Pool(pool);

    let access = r.u16()?;
    let name = pool.class(r.u16()?)?.to_string();
    let super_index = r.u16()?;
    let super_name = if super_index == 0 {
        None
    } else {
        Some(pool.class(super_index)?.to_string())
    };
    let mut interfaces = Vec::new();
    for _ in 0..r.u16()? {
        interfaces.push(pool.class(r.u16()?)?.to_string());
    }
    let fields = members(&mut r, &pool)?;
    let methods = members(&mut r, &pool)?;
    let annotations = attributes(&mut r, &pool)?;

    let mut refs = Vec::new();
    let mut classes = Vec::new();
    for c in &pool.0 {
        match c {
            Const::Ref(kind, class, nat) => {
                let Const::NameAndType(n, d) = pool.get(*nat)? else {
                    return Err("a member reference without a name and type".into());
                };
                refs.push(MemberRef {
                    kind: *kind,
                    owner: pool.class(*class)?.to_string(),
                    name: pool.utf8(*n)?.to_string(),
                    descriptor: pool.utf8(*d)?.to_string(),
                });
            }
            Const::Class(n) => {
                if let Some(e) = element_class(pool.utf8(*n)?) {
                    classes.push(e.to_string());
                }
            }
            _ => {}
        }
    }
    Ok(ClassFile {
        major,
        access,
        name,
        super_name,
        interfaces,
        fields,
        methods,
        refs,
        classes,
        annotations,
    })
}

fn members(r: &mut Reader, pool: &Pool) -> R<Vec<Member>> {
    let mut out = Vec::new();
    for _ in 0..r.u16()? {
        let access = r.u16()?;
        let name = pool.utf8(r.u16()?)?.to_string();
        let descriptor = pool.utf8(r.u16()?)?.to_string();
        let annotations = attributes(r, pool)?;
        out.push(Member {
            name,
            descriptor,
            access,
            annotations,
        });
    }
    Ok(out)
}

/// Skips every attribute, keeping the annotations.
fn attributes(r: &mut Reader, pool: &Pool) -> R<Vec<Annotation>> {
    let mut out = Vec::new();
    for _ in 0..r.u16()? {
        let name = pool.utf8(r.u16()?)?;
        let len = r.u32()? as usize;
        let body = r.bytes(len)?;
        if matches!(
            name,
            "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations"
        ) {
            let mut a = Reader { b: body, at: 0 };
            for _ in 0..a.u16()? {
                out.push(annotation(&mut a, pool, 0)?);
            }
        }
    }
    Ok(out)
}

fn annotation(r: &mut Reader, pool: &Pool, depth: usize) -> R<Annotation> {
    if depth > MAX_DEPTH {
        return Err("annotations nested too deep".into());
    }
    let descriptor = pool.utf8(r.u16()?)?.to_string();
    let mut elements = Vec::new();
    for _ in 0..r.u16()? {
        let name = pool.utf8(r.u16()?)?.to_string();
        elements.push((name, value(r, pool, depth + 1)?));
    }
    Ok(Annotation {
        descriptor,
        elements,
    })
}

fn value(r: &mut Reader, pool: &Pool, depth: usize) -> R<Value> {
    if depth > MAX_DEPTH {
        return Err("annotation values nested too deep".into());
    }
    Ok(match r.u8()? {
        b's' => Value::Str(pool.utf8(r.u16()?)?.to_string()),
        b'B' | b'C' | b'I' | b'S' | b'Z' => match pool.get(r.u16()?)? {
            Const::Int(n) => Value::Int(i64::from(*n)),
            _ => return Err("an int annotation value that is not an integer".into()),
        },
        b'D' | b'F' | b'J' => {
            r.u16()?;
            Value::Other
        }
        b'e' => Value::Enum {
            descriptor: pool.utf8(r.u16()?)?.to_string(),
            name: pool.utf8(r.u16()?)?.to_string(),
        },
        b'c' => Value::Class(pool.utf8(r.u16()?)?.to_string()),
        b'@' => Value::Annotation(annotation(r, pool, depth + 1)?),
        b'[' => {
            let mut v = Vec::new();
            for _ in 0..r.u16()? {
                v.push(value(r, pool, depth + 1)?);
            }
            Value::Array(v)
        }
        t => return Err(format!("unknown annotation value tag {t}")),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny class-file writer, for tests: enough to build the cases the
    /// reader must handle without a Java compiler.
    #[derive(Default)]
    pub struct Builder {
        pool: Vec<Vec<u8>>,
        pub fields: Vec<(String, String)>,
        pub methods: Vec<(String, String)>,
        pub interfaces: Vec<String>,
        refs: Vec<u16>,
        annotations: Vec<Vec<u8>>,
        /// (is a method, index) -> its annotations
        member_annotations: std::collections::HashMap<(bool, usize), Vec<Vec<u8>>>,
    }

    /// An annotation element value, for `Builder::annotation`.
    pub enum Elem<'a> {
        Str(&'a str),
        Strs(Vec<&'a str>),
        Int(i32),
        Ann(Vec<u8>),
    }

    impl Builder {
        /// An annotation's bytes: its type and its elements.
        pub fn annotation(&mut self, descriptor: &str, elements: Vec<(&str, Elem)>) -> Vec<u8> {
            let ty = self.utf8(descriptor);
            let mut a = ty.to_be_bytes().to_vec();
            a.extend((elements.len() as u16).to_be_bytes());
            for (name, v) in elements {
                let n = self.utf8(name);
                a.extend(n.to_be_bytes());
                match v {
                    Elem::Str(s) => {
                        let i = self.utf8(s);
                        a.push(b's');
                        a.extend(i.to_be_bytes());
                    }
                    Elem::Strs(list) => {
                        a.push(b'[');
                        a.extend((list.len() as u16).to_be_bytes());
                        for s in list {
                            let i = self.utf8(s);
                            a.push(b's');
                            a.extend(i.to_be_bytes());
                        }
                    }
                    Elem::Int(v) => {
                        let mut e = vec![3];
                        e.extend(v.to_be_bytes());
                        let i = self.add(e);
                        a.push(b'I');
                        a.extend(i.to_be_bytes());
                    }
                    Elem::Ann(bytes) => {
                        a.push(b'@');
                        a.extend(bytes);
                    }
                }
            }
            a
        }

        /// Puts an annotation on a declared member (`method`: a method, else a
        /// field; `index` in the order they were pushed).
        pub fn annotate(&mut self, method: bool, index: usize, annotation: Vec<u8>) {
            self.member_annotations
                .entry((method, index))
                .or_default()
                .push(annotation);
        }

        fn add(&mut self, entry: Vec<u8>) -> u16 {
            if let Some(i) = self.pool.iter().position(|e| *e == entry) {
                return i as u16 + 1;
            }
            self.pool.push(entry);
            self.pool.len() as u16
        }
        pub fn utf8(&mut self, s: &str) -> u16 {
            let mut e = vec![1];
            e.extend((s.len() as u16).to_be_bytes());
            e.extend(s.as_bytes());
            self.add(e)
        }
        pub fn class(&mut self, name: &str) -> u16 {
            let n = self.utf8(name);
            let mut e = vec![7];
            e.extend(n.to_be_bytes());
            self.add(e)
        }
        pub fn member_ref(&mut self, tag: u8, owner: &str, name: &str, desc: &str) {
            let c = self.class(owner);
            let (n, d) = (self.utf8(name), self.utf8(desc));
            let mut nat = vec![12];
            nat.extend(n.to_be_bytes());
            nat.extend(d.to_be_bytes());
            let nat = self.add(nat);
            let mut e = vec![tag];
            e.extend(c.to_be_bytes());
            e.extend(nat.to_be_bytes());
            let i = self.add(e);
            self.refs.push(i);
        }
        /// `@Mixin(targets = {...})` on the class.
        pub fn mixin_targets(&mut self, targets: &[&str]) {
            let ty = self.utf8("Lorg/spongepowered/asm/mixin/Mixin;");
            let key = self.utf8("targets");
            let mut a = Vec::new();
            a.extend(ty.to_be_bytes());
            a.extend(1u16.to_be_bytes());
            a.extend(key.to_be_bytes());
            a.push(b'[');
            a.extend((targets.len() as u16).to_be_bytes());
            for t in targets {
                let s = self.utf8(t);
                a.push(b's');
                a.extend(s.to_be_bytes());
            }
            self.annotations.push(a);
        }
        pub fn build(mut self, name: &str, super_name: &str) -> Vec<u8> {
            let this = self.class(name);
            let sup = self.class(super_name);
            let interfaces: Vec<u16> = self
                .interfaces
                .clone()
                .iter()
                .map(|i| self.class(i))
                .collect();
            let attr = self.utf8("RuntimeInvisibleAnnotations");
            let members: Vec<(u16, u16)> = self
                .fields
                .clone()
                .iter()
                .chain(self.methods.clone().iter())
                .map(|(n, d)| (self.utf8(n), self.utf8(d)))
                .collect();
            let mut b = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 65];
            b.extend((self.pool.len() as u16 + 1).to_be_bytes());
            for e in &self.pool {
                b.extend(e);
            }
            b.extend(0x21u16.to_be_bytes());
            b.extend(this.to_be_bytes());
            b.extend(sup.to_be_bytes());
            b.extend((interfaces.len() as u16).to_be_bytes());
            for i in &interfaces {
                b.extend(i.to_be_bytes());
            }
            let nf = self.fields.len();
            for (gi, group) in [&members[..nf], &members[nf..]].into_iter().enumerate() {
                b.extend((group.len() as u16).to_be_bytes());
                for (i, (n, d)) in group.iter().enumerate() {
                    b.extend(1u16.to_be_bytes());
                    b.extend(n.to_be_bytes());
                    b.extend(d.to_be_bytes());
                    match self.member_annotations.get(&(gi == 1, i)) {
                        None => b.extend(0u16.to_be_bytes()),
                        Some(list) => {
                            b.extend(1u16.to_be_bytes());
                            b.extend(attr.to_be_bytes());
                            let body: Vec<u8> = (list.len() as u16)
                                .to_be_bytes()
                                .into_iter()
                                .chain(list.concat())
                                .collect();
                            b.extend((body.len() as u32).to_be_bytes());
                            b.extend(body);
                        }
                    }
                }
            }
            if self.annotations.is_empty() {
                b.extend(0u16.to_be_bytes());
            } else {
                b.extend(1u16.to_be_bytes());
                b.extend(attr.to_be_bytes());
                let body: Vec<u8> = (self.annotations.len() as u16)
                    .to_be_bytes()
                    .into_iter()
                    .chain(self.annotations.concat())
                    .collect();
                b.extend((body.len() as u32).to_be_bytes());
                b.extend(body);
            }
            let _ = &self.refs;
            b
        }
    }

    #[test]
    fn reads_what_a_class_declares_and_uses() {
        let mut b = Builder::default();
        b.fields.push(("count".into(), "I".into()));
        b.methods.push(("tick".into(), "()V".into()));
        b.member_ref(10, "net/minecraft/world/level/Level", "isClientSide", "()Z");
        b.member_ref(
            9,
            "net/minecraft/core/BlockPos",
            "ZERO",
            "Lnet/minecraft/core/BlockPos;",
        );
        b.class("[[Lnet/minecraft/world/item/Item;");
        b.mixin_targets(&["net.minecraft.client.Minecraft"]);
        let c = parse(&b.build("demo/Thing", "java/lang/Object")).unwrap();
        assert_eq!(c.name, "demo/Thing");
        assert_eq!(c.super_name.as_deref(), Some("java/lang/Object"));
        assert_eq!(c.fields[0].name, "count");
        assert_eq!(c.methods[0].descriptor, "()V");
        assert!(c.refs.contains(&MemberRef {
            kind: RefKind::Method,
            owner: "net/minecraft/world/level/Level".into(),
            name: "isClientSide".into(),
            descriptor: "()Z".into(),
        }));
        assert!(c
            .refs
            .iter()
            .any(|r| r.kind == RefKind::Field && r.name == "ZERO"));
        assert!(c
            .classes
            .iter()
            .any(|n| n == "net/minecraft/world/item/Item"));
        let m = &c.annotations[0];
        assert_eq!(m.descriptor, "Lorg/spongepowered/asm/mixin/Mixin;");
        assert_eq!(
            m.get("targets"),
            Some(&Value::Array(vec![Value::Str(
                "net.minecraft.client.Minecraft".into()
            )]))
        );
    }

    #[test]
    fn hostile_input_is_an_error_never_a_panic() {
        let mut b = Builder::default();
        b.member_ref(10, "a/B", "c", "()V");
        b.mixin_targets(&["x.Y"]);
        let good = b.build("a/A", "java/lang/Object");
        // every truncation, and every single byte changed to every value at a
        // few positions spread over the file
        for n in 0..good.len() {
            let _ = parse(&good[..n]);
        }
        let step = (good.len() / 40).max(1);
        for at in (0..good.len()).step_by(step) {
            for v in 0..=255u8 {
                let mut bad = good.clone();
                bad[at] = v;
                let _ = parse(&bad);
            }
        }
        assert!(parse(b"not a class").is_err());
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn array_classes_name_their_element() {
        assert_eq!(element_class("[[Lnet/a/B;"), Some("net/a/B"));
        assert_eq!(element_class("[I"), None);
        assert_eq!(element_class("net/a/B"), Some("net/a/B"));
    }
}
