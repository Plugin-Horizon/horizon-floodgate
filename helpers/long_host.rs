fn floodgate_long_host(
    reader: &mut crate::buf::Reader<'_>,
    limit: usize,
) -> core::ops::ControlFlow<Result<String>> {
    if limit == 255 {
        return core::ops::ControlFlow::Break(reader.string(32767));
    }
    core::ops::ControlFlow::Continue(())
}
