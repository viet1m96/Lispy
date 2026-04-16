; Largest palindrome made from products of 2 3-digits numbers


(defun rev3 (n)
  (let ((d0 (% n 10))
        (d1 (% (/ n 10) 10))
        (d2 (/ n 100)))
    (+ (* d0 100) (* d1 10) d2)))

(defun make-pal6 (left)
  (+ (* left 1000) (rev3 left)))

(begin
  (setq ans 0)
  (setq left 999)

  (loop while (and (= ans 0) (>= left 100)) do
    (begin
      (setq p (make-pal6 left))
      (setq a 990)

      (loop while (and (= ans 0) (>= a 110) (>= (* a 999) p)) do
        (begin
          (if (= (% p a) 0)
              (begin
                (setq b (/ p a))
                (if (and (>= b 100) (<= b 999))
                    (setq ans p)
                    0))
              0)
          (setq a (- a 11)))
        finally 0)

      (setq left (- left 1)))
    finally ans)

  (print ans)
  (halt))
