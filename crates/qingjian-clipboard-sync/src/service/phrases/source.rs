//! 壳提供本机短语快照；同步层负责收件箱与加密传输。

pub trait PhraseSource: Send {
    fn snapshot(&self) -> std::io::Result<Vec<qingjian_learning::Phrase>>;
}

impl<F> PhraseSource for F
where
    F: Fn() -> std::io::Result<Vec<qingjian_learning::Phrase>> + Send,
{
    fn snapshot(&self) -> std::io::Result<Vec<qingjian_learning::Phrase>> {
        self()
    }
}
