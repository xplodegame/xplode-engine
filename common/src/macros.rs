#[macro_export]
macro_rules! agg_mod {
    [ $( $name:ident $(,)? )+ ] => {
        $(
            pub mod $name;
        )+
    };
}

#[macro_export]
macro_rules! impl_from_str_for_enum {
    ($enum_name:ident, $( $variant:ident ),*) => {
        impl std::str::FromStr for $enum_name {
            type Err = anyhow::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s.to_uppercase().as_str() {
                    $(stringify!($variant) => Ok($enum_name::$variant),)*
                    _ => Err(anyhow::anyhow!("Invalid variant: {}", s)),
                }
            }
        }
    };
}

#[macro_export]
macro_rules! impl_to_string_for_enum {
    ($enum_name:ident, $( $variant:ident ),*) => {
        impl ToString for $enum_name {
            fn to_string(&self) -> String {
                match self {
                    $( $enum_name::$variant => stringify!($variant).to_string(), )*
                }
            }
        }
    };
}
