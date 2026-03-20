use fusion_std::thread::GENERATED_EXPLICIT_FIBER_TASK_ROOTS;

fn main() {
    for root in GENERATED_EXPLICIT_FIBER_TASK_ROOTS {
        println!("{} = {}, {}", root.type_name, root.symbol, root.priority);
    }
}
