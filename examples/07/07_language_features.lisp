; Basic features of language

(begin
  (setq x 5)
  (print (if (> x 3) 111 222))
  (print-str "\n")

  (print
    (let ((a 10)
          (b 20))
      (+ a b)))
  (print-str "\n")

  (setq sum 0)
  (setq i 1)
  (loop while (<= i 5) do
    (setq sum (+ sum i))
    (setq i (+ i 1))
  finally sum)
  (print sum)
  (halt))
