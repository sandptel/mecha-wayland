// Here we define macros that wrap puffin's profiling macros. When the "profile" feature is enabled, these macros will call the corresponding puffin macros.
// When the "profile" feature is disabled, they will expand to nothing, ensuring zero overhead in production builds.

#[cfg(feature = "profile")]
#[macro_export]
macro_rules! profile_function {
    ($($arg:tt)*) => {
        puffin::profile_function!($($arg)*);
    };
}

#[cfg(not(feature = "profile"))]
#[macro_export]
macro_rules! profile_function {
    ($($arg:tt)*) => {};
}

#[cfg(feature = "profile")]
#[macro_export]
macro_rules! profile_scope {
    ($($arg:tt)*) => {
        puffin::profile_scope!($($arg)*);
    };
}

#[cfg(not(feature = "profile"))]
#[macro_export]
macro_rules! profile_scope {
    ($($arg:tt)*) => {};
}
