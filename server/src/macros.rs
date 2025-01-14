#[macro_export]
macro_rules! agg_mod {
    [ $( $name:ident $(,)? )+ ] => {
        $(
            pub mod $name;
        )+
    };
}
