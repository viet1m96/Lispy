; Bubble sort
(begin
  (setq s "43152")
  (setq n (strlen s))
  (setq i 0)

  (loop while (< i n) do
    (setq j 0)
    (loop while (< j (- n 1)) do
      (if (> (strget s j) (strget s (+ j 1)))
          (let ((tmp (strget s j)))
            (begin
              (strset s j (strget s (+ j 1)))
              (strset s (+ j 1) tmp)))
          0)
      (setq j (+ j 1))
    finally 0)
    (setq i (+ i 1))
  finally 0)

  (print-str s)
  (halt))
