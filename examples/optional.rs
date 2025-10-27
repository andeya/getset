use getset2::Getset2;

#[derive(Default, Getset2)]
pub struct Foo {
    #[getset2(set(into), get_ref(as_ref))]
    optional: Option<String>,
}

// cargo expand --example optional

fn main() {
    let mut foo = Foo::default();
    foo.set_optional("hello".to_string());
    assert_eq!(foo.optional(), Some(&"hello".to_string()));
}
